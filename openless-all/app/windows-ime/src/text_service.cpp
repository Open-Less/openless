#include "text_service.h"

#include <cstring>
#include <new>
#include <utility>

#include "edit_session.h"

extern LONG g_object_count;
extern HINSTANCE g_module;

namespace {

constexpr wchar_t kMessageWindowClassName[] = L"OpenLessImeMessageWindow";
constexpr UINT kRunSubmitMessage = WM_APP + 1;

// WM_COPYDATA protocol, mirrored in src-tauri/src/windows_ime_protocol.rs.
//
// UIPI is left in place on purpose: the window never opts in to WM_COPYDATA
// from lower-integrity senders, so a non-elevated process cannot inject text
// into an elevated host through this DLL.
constexpr ULONG_PTR kCopyDataSubmit = 0x4F4C5331; // "OLS1": uint32 token + UTF-16LE text
constexpr ULONG_PTR kCopyDataQuery = 0x4F4C5131;  // "OLQ1": uint32 token
constexpr DWORD kMaxSubmitBytes = 1024 * 1024;

// Replies are nonzero so they differ from an unhandled message (0). A failed
// commit is reported as its HRESULT, which always has the high bit set.
constexpr LRESULT kStatusAccepted = 0x4F4C0001;
constexpr LRESULT kStatusPending = 0x4F4C0002;
constexpr LRESULT kStatusCommitted = 0x4F4C0003;
constexpr LRESULT kStatusUnknownToken = 0x4F4C0004;
constexpr LRESULT kStatusBadRequest = 0x4F4C0005;

LRESULT StatusFromHResult(HRESULT hr) {
  return SUCCEEDED(hr) ? kStatusCommitted : static_cast<LRESULT>(hr);
}

bool ReadToken(const COPYDATASTRUCT *copy_data, uint32_t *token) {
  if (copy_data->lpData == nullptr || copy_data->cbData < sizeof(uint32_t)) {
    return false;
  }
  std::memcpy(token, copy_data->lpData, sizeof(uint32_t));
  return *token != 0;
}

} // namespace

OpenLessTextService::OpenLessTextService() { InterlockedIncrement(&g_object_count); }

OpenLessTextService::~OpenLessTextService() {
  Deactivate();
  InterlockedDecrement(&g_object_count);
}

STDMETHODIMP OpenLessTextService::QueryInterface(REFIID iid, void **object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (iid == IID_IUnknown || iid == IID_ITfTextInputProcessor ||
      iid == IID_ITfTextInputProcessorEx) {
    *object = static_cast<ITfTextInputProcessorEx *>(this);
    AddRef();
    return S_OK;
  }

  return E_NOINTERFACE;
}

STDMETHODIMP_(ULONG) OpenLessTextService::AddRef() {
  return static_cast<ULONG>(InterlockedIncrement(&ref_count_));
}

STDMETHODIMP_(ULONG) OpenLessTextService::Release() {
  const ULONG count = static_cast<ULONG>(InterlockedDecrement(&ref_count_));
  if (count == 0) {
    delete this;
  }
  return count;
}

STDMETHODIMP OpenLessTextService::Activate(ITfThreadMgr *thread_mgr, TfClientId client_id) {
  return ActivateEx(thread_mgr, client_id, 0);
}

STDMETHODIMP OpenLessTextService::ActivateEx(ITfThreadMgr *thread_mgr, TfClientId client_id,
                                             DWORD flags) {
  UNREFERENCED_PARAMETER(flags);

  if (thread_mgr == nullptr) {
    return E_INVALIDARG;
  }

  Deactivate();

  thread_mgr_ = thread_mgr;
  thread_mgr_->AddRef();
  client_id_ = client_id;

  const HRESULT hr = EnsureMessageWindow();
  if (FAILED(hr)) {
    Deactivate();
    return hr;
  }

  return S_OK;
}

// Must stay wait-free: TSF can call this while the host thread is being torn
// down, where blocking on another thread deadlocks the whole host process.
STDMETHODIMP OpenLessTextService::Deactivate() {
  CancelPendingSubmit();
  DestroyMessageWindow();

  if (thread_mgr_ != nullptr) {
    thread_mgr_->Release();
    thread_mgr_ = nullptr;
  }
  client_id_ = TF_CLIENTID_NULL;

  return S_OK;
}

HRESULT OpenLessTextService::EnsureMessageWindow() {
  if (message_window_ != nullptr) {
    return S_OK;
  }

  WNDCLASSW window_class = {};
  window_class.lpfnWndProc = OpenLessTextService::MessageWindowProc;
  window_class.hInstance = g_module;
  window_class.lpszClassName = kMessageWindowClassName;

  if (!RegisterClassW(&window_class)) {
    const DWORD error = GetLastError();
    if (error != ERROR_CLASS_ALREADY_EXISTS) {
      return HRESULT_FROM_WIN32(error);
    }
  }

  message_window_ = CreateWindowExW(0, kMessageWindowClassName, L"", 0, 0, 0, 0, 0, HWND_MESSAGE,
                                    nullptr, g_module, this);
  if (message_window_ == nullptr) {
    return HRESULT_FROM_WIN32(GetLastError());
  }

  return S_OK;
}

void OpenLessTextService::DestroyMessageWindow() {
  if (message_window_ != nullptr) {
    const HWND window = message_window_;
    message_window_ = nullptr;
    SetWindowLongPtrW(window, GWLP_USERDATA, 0);
    DestroyWindow(window);
  }
}

LRESULT OpenLessTextService::HandleCopyData(const COPYDATASTRUCT *copy_data) {
  if (copy_data == nullptr) {
    return kStatusBadRequest;
  }
  if (copy_data->dwData == kCopyDataSubmit) {
    return AcceptSubmit(copy_data);
  }
  if (copy_data->dwData == kCopyDataQuery) {
    return QuerySubmit(copy_data);
  }
  return kStatusBadRequest;
}

LRESULT OpenLessTextService::AcceptSubmit(const COPYDATASTRUCT *copy_data) {
  uint32_t token = 0;
  if (!ReadToken(copy_data, &token) || copy_data->cbData > kMaxSubmitBytes ||
      (copy_data->cbData - sizeof(uint32_t)) % sizeof(wchar_t) != 0) {
    return kStatusBadRequest;
  }

  CancelPendingSubmit();

  const size_t text_bytes = copy_data->cbData - sizeof(uint32_t);
  submit_text_.assign(text_bytes / sizeof(wchar_t), L'\0');
  std::memcpy(submit_text_.data(), static_cast<const BYTE *>(copy_data->lpData) + sizeof(uint32_t),
              text_bytes);
  submit_token_ = token;
  submit_status_ = kStatusPending;

  // The sender's SendMessage can be dispatched while the host is in the middle
  // of something else. Commit from a posted message instead, so the edit
  // session is requested from the top of the host message loop.
  if (!PostMessageW(message_window_, kRunSubmitMessage, token, 0)) {
    const DWORD error = GetLastError();
    CancelPendingSubmit();
    return StatusFromHResult(HRESULT_FROM_WIN32(error != ERROR_SUCCESS ? error : ERROR_GEN_FAILURE));
  }
  return kStatusAccepted;
}

LRESULT OpenLessTextService::QuerySubmit(const COPYDATASTRUCT *copy_data) {
  uint32_t token = 0;
  if (!ReadToken(copy_data, &token)) {
    return kStatusBadRequest;
  }
  if (token != submit_token_) {
    return kStatusUnknownToken;
  }

  if (async_edit_ && async_edit_->completed) {
    submit_status_ = StatusFromHResult(async_edit_->result);
    async_edit_.reset();
  }
  return submit_status_;
}

void OpenLessTextService::RunPendingSubmit(uint32_t token) {
  if (token != submit_token_ || submit_status_ != kStatusPending || async_edit_) {
    return; // Superseded, cancelled, or already running.
  }

  std::shared_ptr<OpenLessAsyncEditState> async_edit;
  HRESULT hr = E_UNEXPECTED;
  try {
    const std::wstring text = std::move(submit_text_);
    submit_text_.clear();
    hr = CommitTextOnOwnerThread(text, &async_edit);
  } catch (const std::bad_alloc &) {
    hr = E_OUTOFMEMORY;
  } catch (...) {
    hr = E_UNEXPECTED;
  }

  // The edit session can re-enter the message loop, so a newer submit or
  // Deactivate may have replaced this one in the meantime.
  if (token != submit_token_) {
    if (async_edit) {
      async_edit->cancelled = true;
    }
    return;
  }

  if (SUCCEEDED(hr) && async_edit) {
    async_edit_ = std::move(async_edit); // QuerySubmit reports it once TSF runs the session.
    return;
  }
  submit_status_ = StatusFromHResult(hr);
}

void OpenLessTextService::CancelPendingSubmit() {
  if (async_edit_) {
    async_edit_->cancelled = true;
    async_edit_.reset();
  }
  submit_text_.clear();
  submit_token_ = 0;
  submit_status_ = 0;
}

HRESULT OpenLessTextService::CommitTextOnOwnerThread(
    const std::wstring &text, std::shared_ptr<OpenLessAsyncEditState> *async_edit) {
  if (thread_mgr_ == nullptr || client_id_ == TF_CLIENTID_NULL) {
    return E_UNEXPECTED;
  }

  ITfDocumentMgr *document_mgr = nullptr;
  HRESULT hr = thread_mgr_->GetFocus(&document_mgr);
  if (FAILED(hr)) {
    return hr;
  }
  if (document_mgr == nullptr) {
    return E_FAIL;
  }

  ITfContext *context = nullptr;
  hr = document_mgr->GetTop(&context);
  document_mgr->Release();
  document_mgr = nullptr;
  if (FAILED(hr)) {
    return hr;
  }
  if (context == nullptr) {
    return E_FAIL;
  }

  const TfClientId client_id = client_id_;
  auto *session = new (std::nothrow) OpenLessEditSession(context, text);
  if (session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT edit_result = S_OK;
  hr = context->RequestEditSession(client_id, session, TF_ES_SYNC | TF_ES_READWRITE, &edit_result);
  session->Release();

  const bool synchronous_rejected =
      hr == TF_E_SYNCHRONOUS || (SUCCEEDED(hr) && edit_result == TF_E_SYNCHRONOUS);
  if (!synchronous_rejected) {
    context->Release();
    if (FAILED(hr)) {
      return hr;
    }
    return edit_result;
  }

  // Hosts such as Word refuse a synchronous lock; queue the edit instead and
  // let the caller poll for its completion.
  auto completion = std::make_shared<OpenLessAsyncEditState>();
  auto *async_session = new (std::nothrow) OpenLessEditSession(context, text, completion);
  if (async_session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT async_edit_result = S_OK;
  hr = context->RequestEditSession(client_id, async_session, TF_ES_ASYNC | TF_ES_READWRITE,
                                   &async_edit_result);
  async_session->Release();
  context->Release();

  if (FAILED(hr)) {
    return hr;
  }
  if (FAILED(async_edit_result)) {
    return async_edit_result;
  }

  *async_edit = std::move(completion);
  return S_OK;
}

LRESULT CALLBACK OpenLessTextService::MessageWindowProc(HWND window, UINT message, WPARAM wparam,
                                                        LPARAM lparam) {
  if (message == WM_NCCREATE) {
    const auto *create = reinterpret_cast<CREATESTRUCTW *>(lparam);
    SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(create->lpCreateParams));
    return TRUE;
  }

  auto *service = reinterpret_cast<OpenLessTextService *>(GetWindowLongPtrW(window, GWLP_USERDATA));
  if (service == nullptr || (message != WM_COPYDATA && message != kRunSubmitMessage)) {
    return DefWindowProcW(window, message, wparam, lparam);
  }

  // Keep the service alive: committing can re-enter and let TSF deactivate and
  // release it before this call returns.
  service->AddRef();
  LRESULT result = 0;
  try {
    if (message == WM_COPYDATA) {
      result = service->HandleCopyData(reinterpret_cast<const COPYDATASTRUCT *>(lparam));
    } else {
      service->RunPendingSubmit(static_cast<uint32_t>(wparam));
    }
  } catch (const std::bad_alloc &) {
    // Never let an allocation or STL exception terminate the host process.
    result = StatusFromHResult(E_OUTOFMEMORY);
  } catch (...) {
    result = StatusFromHResult(E_UNEXPECTED);
  }
  service->Release();
  return result;
}
