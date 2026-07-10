#include "text_service.h"

#include <memory>
#include <new>

#include "edit_session.h"

extern LONG g_object_count;
extern HINSTANCE g_module;

namespace {

constexpr wchar_t kMessageWindowClassName[] = L"OpenLessImeMessageWindow";
constexpr UINT kSubmitTextMessage = WM_APP + 1;
constexpr UINT kSubmitTextTimeoutMs = 2000;

struct SubmitTextRequest {
  const std::wstring* session_id = nullptr;
  const std::wstring* text = nullptr;
  std::shared_ptr<OpenLessAsyncEditState> async_completion;
  bool wait_for_async_completion = false;
  HRESULT result = E_UNEXPECTED;
};

HRESULT WaitForAsyncEditCompletion(
    const std::shared_ptr<OpenLessAsyncEditState>& completion) {
  if (!completion || !completion->IsValid()) {
    return HRESULT_FROM_WIN32(completion && completion->create_error != ERROR_SUCCESS
                                  ? completion->create_error
                                  : ERROR_INVALID_HANDLE);
  }

  const DWORD wait_result =
      WaitForSingleObject(completion->event, kSubmitTextTimeoutMs);
  if (wait_result == WAIT_OBJECT_0) {
    return completion->result;
  }
  if (wait_result == WAIT_TIMEOUT) {
    return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
  }
  return HRESULT_FROM_WIN32(GetLastError());
}

}  // namespace

OpenLessTextService::OpenLessTextService() {
  InterlockedIncrement(&g_object_count);
}

OpenLessTextService::~OpenLessTextService() {
  Deactivate();
  InterlockedDecrement(&g_object_count);
}

STDMETHODIMP OpenLessTextService::QueryInterface(REFIID iid, void** object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (iid == IID_IUnknown || iid == IID_ITfTextInputProcessor ||
      iid == IID_ITfTextInputProcessorEx) {
    *object = static_cast<ITfTextInputProcessorEx*>(this);
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

STDMETHODIMP OpenLessTextService::Activate(ITfThreadMgr* thread_mgr,
                                           TfClientId client_id) {
  return ActivateEx(thread_mgr, client_id, 0);
}

STDMETHODIMP OpenLessTextService::ActivateEx(ITfThreadMgr* thread_mgr,
                                             TfClientId client_id,
                                             DWORD flags) {
  UNREFERENCED_PARAMETER(flags);

  if (thread_mgr == nullptr) {
    return E_INVALIDARG;
  }

  Deactivate();

  owner_thread_id_ = GetCurrentThreadId();

  thread_mgr_ = thread_mgr;
  thread_mgr_->AddRef();
  client_id_ = client_id;

  HRESULT hr = EnsureMessageWindow();
  if (FAILED(hr)) {
    Deactivate();
    return hr;
  }

  hr = StartIpcServer();
  if (FAILED(hr)) {
    Deactivate();
    return hr;
  }

  return S_OK;
}

STDMETHODIMP OpenLessTextService::Deactivate() {
  StopIpcServer();
  DestroyMessageWindow();

  if (thread_mgr_ != nullptr) {
    thread_mgr_->Release();
    thread_mgr_ = nullptr;
  }
  client_id_ = TF_CLIENTID_NULL;
  owner_thread_id_ = 0;

  return S_OK;
}

HRESULT OpenLessTextService::SubmitTextFromPipe(
    const std::wstring& session_id,
    const std::wstring& text) {
  if (GetCurrentThreadId() == owner_thread_id_) {
    return CommitTextOnOwnerThread(session_id, text, nullptr, nullptr);
  }

  if (message_window_ == nullptr) {
    return E_UNEXPECTED;
  }

  SubmitTextRequest request;
  request.session_id = &session_id;
  request.text = &text;
  DWORD_PTR message_result = 0;
  const LRESULT sent = SendMessageTimeoutW(
      message_window_, kSubmitTextMessage, 0,
      reinterpret_cast<LPARAM>(&request), SMTO_ABORTIFHUNG,
      kSubmitTextTimeoutMs, &message_result);
  if (sent == 0) {
    const DWORD error = GetLastError();
    return HRESULT_FROM_WIN32(error != ERROR_SUCCESS ? error : ERROR_TIMEOUT);
  }

  if (request.wait_for_async_completion) {
    return WaitForAsyncEditCompletion(request.async_completion);
  }

  return request.result;
}

HRESULT OpenLessTextService::StartIpcServer() {
  pipe_server_.Start(this);
  return S_OK;
}

void OpenLessTextService::StopIpcServer() {
  pipe_server_.Stop();
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

  message_window_ =
      CreateWindowExW(0, kMessageWindowClassName, L"", 0, 0, 0, 0, 0,
                      HWND_MESSAGE, nullptr, g_module, this);
  if (message_window_ == nullptr) {
    return HRESULT_FROM_WIN32(GetLastError());
  }

  return S_OK;
}

void OpenLessTextService::DestroyMessageWindow() {
  if (message_window_ != nullptr) {
    DestroyWindow(message_window_);
    message_window_ = nullptr;
  }
}

HRESULT OpenLessTextService::CommitTextOnOwnerThread(
    const std::wstring& session_id,
    const std::wstring& text,
    std::shared_ptr<OpenLessAsyncEditState>* async_completion,
    bool* wait_for_async_completion) {
  UNREFERENCED_PARAMETER(session_id);

  if (thread_mgr_ == nullptr || client_id_ == TF_CLIENTID_NULL) {
    return E_UNEXPECTED;
  }

  ITfDocumentMgr* document_mgr = nullptr;
  HRESULT hr = thread_mgr_->GetFocus(&document_mgr);
  if (FAILED(hr)) {
    return hr;
  }
  if (document_mgr == nullptr) {
    return E_FAIL;
  }

  ITfContext* context = nullptr;
  hr = document_mgr->GetTop(&context);
  document_mgr->Release();
  document_mgr = nullptr;
  if (FAILED(hr)) {
    return hr;
  }
  if (context == nullptr) {
    return E_FAIL;
  }

  auto* session = new (std::nothrow) OpenLessEditSession(context, text);
  if (session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT edit_result = S_OK;
  hr = context->RequestEditSession(client_id_, session,
                                   TF_ES_SYNC | TF_ES_READWRITE, &edit_result);
  session->Release();

  const bool synchronous_rejected =
      hr == TF_E_SYNCHRONOUS ||
      (SUCCEEDED(hr) && edit_result == TF_E_SYNCHRONOUS);
  if (!synchronous_rejected) {
    context->Release();
    if (FAILED(hr)) {
      return hr;
    }
    return edit_result;
  }

  if (async_completion == nullptr || wait_for_async_completion == nullptr) {
    context->Release();
    if (FAILED(hr)) {
      return hr;
    }
    return edit_result;
  }

  auto completion = std::make_shared<OpenLessAsyncEditState>();
  if (!completion->IsValid()) {
    context->Release();
    return HRESULT_FROM_WIN32(completion->create_error != ERROR_SUCCESS
                                  ? completion->create_error
                                  : ERROR_INVALID_HANDLE);
  }

  auto* async_session =
      new (std::nothrow) OpenLessEditSession(context, text, completion);
  if (async_session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT async_edit_result = S_OK;
  hr = context->RequestEditSession(client_id_, async_session,
                                   TF_ES_ASYNC | TF_ES_READWRITE,
                                   &async_edit_result);
  async_session->Release();
  context->Release();

  if (FAILED(hr)) {
    return hr;
  }
  if (FAILED(async_edit_result)) {
    return async_edit_result;
  }

  *async_completion = std::move(completion);
  *wait_for_async_completion = true;
  return S_OK;
}

LRESULT CALLBACK OpenLessTextService::MessageWindowProc(HWND window,
                                                        UINT message,
                                                        WPARAM wparam,
                                                        LPARAM lparam) {
  UNREFERENCED_PARAMETER(wparam);

  if (message == WM_NCCREATE) {
    const auto* create = reinterpret_cast<CREATESTRUCTW*>(lparam);
    SetWindowLongPtrW(window, GWLP_USERDATA,
                      reinterpret_cast<LONG_PTR>(create->lpCreateParams));
    return TRUE;
  }

  auto* service = reinterpret_cast<OpenLessTextService*>(
      GetWindowLongPtrW(window, GWLP_USERDATA));
  if (message == kSubmitTextMessage && service != nullptr) {
    auto* request = reinterpret_cast<SubmitTextRequest*>(lparam);
    if (request == nullptr || request->session_id == nullptr ||
        request->text == nullptr) {
      return 0;
    }

    request->result = service->CommitTextOnOwnerThread(
        *request->session_id, *request->text, &request->async_completion,
        &request->wait_for_async_completion);
    return 1;
  }

  return DefWindowProcW(window, message, wparam, lparam);
}
