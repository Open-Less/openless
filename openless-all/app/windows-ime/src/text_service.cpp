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
  SubmitTextRequest()
      : cancellation(std::make_shared<std::atomic<bool>>(false)),
        completion_event(CreateEventW(nullptr, TRUE, FALSE, nullptr)) {
    if (completion_event == nullptr) {
      create_error = GetLastError();
    }
  }

  ~SubmitTextRequest() {
    if (completion_event != nullptr) {
      CloseHandle(completion_event);
      completion_event = nullptr;
    }
  }

  bool IsValid() const { return completion_event != nullptr; }

  std::wstring session_id;
  std::wstring text;
  std::shared_ptr<OpenLessAsyncEditState> async_completion;
  bool wait_for_async_completion = false;
  HRESULT result = E_UNEXPECTED;
  std::shared_ptr<std::atomic<bool>> cancellation;
  HANDLE completion_event = nullptr;
  DWORD create_error = ERROR_SUCCESS;
};

using PostedSubmitRequest = std::shared_ptr<SubmitTextRequest>;

HRESULT WaitForCompletionOrCancellation(HANDLE completion_event, HANDLE cancellation_event,
                                        const std::shared_ptr<std::atomic<bool>> &cancellation) {
  if (completion_event == nullptr) {
    return HRESULT_FROM_WIN32(ERROR_INVALID_HANDLE);
  }
  if (cancellation_event != nullptr &&
      WaitForSingleObject(cancellation_event, 0) == WAIT_OBJECT_0) {
    cancellation->store(true);
    return HRESULT_FROM_WIN32(ERROR_CANCELLED);
  }

  const HANDLE wait_handles[2] = {completion_event, cancellation_event};
  const DWORD wait_count = cancellation_event != nullptr ? 2 : 1;
  const DWORD wait_result =
      WaitForMultipleObjects(wait_count, wait_handles, FALSE, kSubmitTextTimeoutMs);
  if (wait_result == WAIT_OBJECT_0) {
    return S_OK;
  }

  cancellation->store(true);
  if (wait_count == 2 && wait_result == WAIT_OBJECT_0 + 1) {
    return HRESULT_FROM_WIN32(ERROR_CANCELLED);
  }
  if (wait_result == WAIT_TIMEOUT) {
    return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
  }
  const DWORD error = GetLastError();
  return HRESULT_FROM_WIN32(error != ERROR_SUCCESS ? error : ERROR_GEN_FAILURE);
}

HRESULT WaitForAsyncEditCompletion(const std::shared_ptr<OpenLessAsyncEditState> &completion,
                                   HANDLE cancellation_event,
                                   const std::shared_ptr<std::atomic<bool>> &cancellation) {
  if (!completion || !completion->IsValid()) {
    return HRESULT_FROM_WIN32(completion && completion->create_error != ERROR_SUCCESS
                                  ? completion->create_error
                                  : ERROR_INVALID_HANDLE);
  }
  const HRESULT wait_result =
      WaitForCompletionOrCancellation(completion->event, cancellation_event, cancellation);
  return FAILED(wait_result) ? wait_result : completion->result;
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

HRESULT OpenLessTextService::SubmitTextFromPipe(const std::wstring &session_id,
                                                const std::wstring &text,
                                                HANDLE cancellation_event) {
  try {
    if (GetCurrentThreadId() == owner_thread_id_) {
      auto cancellation = std::make_shared<std::atomic<bool>>(false);
      return CommitTextOnOwnerThread(session_id, text, nullptr, nullptr, cancellation);
    }

    if (message_window_ == nullptr) {
      return E_UNEXPECTED;
    }

    auto request = std::make_shared<SubmitTextRequest>();
    if (!request->IsValid()) {
      return HRESULT_FROM_WIN32(request->create_error != ERROR_SUCCESS ? request->create_error
                                                                       : ERROR_INVALID_HANDLE);
    }
    request->session_id = session_id;
    request->text = text;

    auto *posted_request = new (std::nothrow) PostedSubmitRequest(request);
    if (posted_request == nullptr) {
      return E_OUTOFMEMORY;
    }

    if (!PostMessageW(message_window_, kSubmitTextMessage, 0,
                      reinterpret_cast<LPARAM>(posted_request))) {
      const DWORD error = GetLastError();
      delete posted_request;
      return HRESULT_FROM_WIN32(error != ERROR_SUCCESS ? error : ERROR_GEN_FAILURE);
    }

    const HRESULT wait_result = WaitForCompletionOrCancellation(
        request->completion_event, cancellation_event, request->cancellation);
    if (FAILED(wait_result)) {
      return wait_result;
    }

    if (request->wait_for_async_completion) {
      return WaitForAsyncEditCompletion(request->async_completion, cancellation_event,
                                        request->cancellation);
    }
    return request->result;
  } catch (const std::bad_alloc &) {
    return E_OUTOFMEMORY;
  } catch (...) {
    return E_UNEXPECTED;
  }
}

HRESULT OpenLessTextService::StartIpcServer() { return pipe_server_.Start(this); }

void OpenLessTextService::StopIpcServer() { pipe_server_.Stop(); }

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
    MSG message = {};
    while (PeekMessageW(&message, message_window_, kSubmitTextMessage, kSubmitTextMessage,
                        PM_REMOVE)) {
      delete reinterpret_cast<PostedSubmitRequest *>(message.lParam);
    }
    DestroyWindow(message_window_);
    message_window_ = nullptr;
  }
}

HRESULT OpenLessTextService::CommitTextOnOwnerThread(
    const std::wstring &session_id, const std::wstring &text,
    std::shared_ptr<OpenLessAsyncEditState> *async_completion, bool *wait_for_async_completion,
    const std::shared_ptr<std::atomic<bool>> &cancellation) {
  UNREFERENCED_PARAMETER(session_id);

  if (thread_mgr_ == nullptr || client_id_ == TF_CLIENTID_NULL) {
    return E_UNEXPECTED;
  }
  if (cancellation && cancellation->load()) {
    return HRESULT_FROM_WIN32(ERROR_CANCELLED);
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

  auto *session = new (std::nothrow) OpenLessEditSession(context, text, nullptr, cancellation);
  if (session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT edit_result = S_OK;
  hr = context->RequestEditSession(client_id_, session, TF_ES_SYNC | TF_ES_READWRITE, &edit_result);
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

  if (async_completion == nullptr || wait_for_async_completion == nullptr) {
    context->Release();
    if (FAILED(hr)) {
      return hr;
    }
    return edit_result;
  }

  if (cancellation && cancellation->load()) {
    context->Release();
    return HRESULT_FROM_WIN32(ERROR_CANCELLED);
  }

  auto completion = std::make_shared<OpenLessAsyncEditState>();
  if (!completion->IsValid()) {
    context->Release();
    return HRESULT_FROM_WIN32(completion->create_error != ERROR_SUCCESS ? completion->create_error
                                                                        : ERROR_INVALID_HANDLE);
  }

  auto *async_session =
      new (std::nothrow) OpenLessEditSession(context, text, completion, cancellation);
  if (async_session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT async_edit_result = S_OK;
  hr = context->RequestEditSession(client_id_, async_session, TF_ES_ASYNC | TF_ES_READWRITE,
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

LRESULT CALLBACK OpenLessTextService::MessageWindowProc(HWND window, UINT message, WPARAM wparam,
                                                        LPARAM lparam) {
  UNREFERENCED_PARAMETER(wparam);

  if (message == WM_NCCREATE) {
    const auto *create = reinterpret_cast<CREATESTRUCTW *>(lparam);
    SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(create->lpCreateParams));
    return TRUE;
  }

  auto *service = reinterpret_cast<OpenLessTextService *>(GetWindowLongPtrW(window, GWLP_USERDATA));
  if (message == kSubmitTextMessage && service != nullptr) {
    std::unique_ptr<PostedSubmitRequest> posted_request(
        reinterpret_cast<PostedSubmitRequest *>(lparam));
    if (!posted_request || !*posted_request) {
      return 0;
    }

    const auto request = *posted_request;
    if (request->cancellation->load()) {
      request->result = HRESULT_FROM_WIN32(ERROR_CANCELLED);
    } else {
      try {
        request->result = service->CommitTextOnOwnerThread(
            request->session_id, request->text, &request->async_completion,
            &request->wait_for_async_completion, request->cancellation);
      } catch (const std::bad_alloc &) {
        request->result = E_OUTOFMEMORY;
      } catch (...) {
        request->result = E_UNEXPECTED;
      }
    }
    SetEvent(request->completion_event);
    return 1;
  }

  return DefWindowProcW(window, message, wparam, lparam);
}
