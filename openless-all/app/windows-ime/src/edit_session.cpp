#include "edit_session.h"

#include <utility>

extern LONG g_object_count;

OpenLessAsyncEditState::OpenLessAsyncEditState()
    : event(CreateEventW(nullptr, TRUE, FALSE, nullptr)) {
  if (event == nullptr) {
    create_error = GetLastError();
  }
}

OpenLessAsyncEditState::~OpenLessAsyncEditState() {
  if (event != nullptr) {
    CloseHandle(event);
    event = nullptr;
  }
}

bool OpenLessAsyncEditState::IsValid() const {
  return event != nullptr;
}

OpenLessEditSession::OpenLessEditSession(
    ITfContext* context,
    std::wstring text,
    std::shared_ptr<OpenLessAsyncEditState> async_state,
    std::shared_ptr<std::atomic<bool>> cancellation)
    : context_(context),
      text_(std::move(text)),
      async_state_(std::move(async_state)),
      cancellation_(std::move(cancellation)) {
  InterlockedIncrement(&g_object_count);
  if (context_ != nullptr) {
    context_->AddRef();
  }
}

OpenLessEditSession::~OpenLessEditSession() {
  if (context_ != nullptr) {
    context_->Release();
    context_ = nullptr;
  }
  InterlockedDecrement(&g_object_count);
}

STDMETHODIMP OpenLessEditSession::QueryInterface(REFIID iid, void** object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (iid == IID_IUnknown || iid == IID_ITfEditSession) {
    *object = static_cast<ITfEditSession*>(this);
    AddRef();
    return S_OK;
  }

  return E_NOINTERFACE;
}

STDMETHODIMP_(ULONG) OpenLessEditSession::AddRef() {
  return static_cast<ULONG>(InterlockedIncrement(&ref_count_));
}

STDMETHODIMP_(ULONG) OpenLessEditSession::Release() {
  const ULONG count = static_cast<ULONG>(InterlockedDecrement(&ref_count_));
  if (count == 0) {
    delete this;
  }
  return count;
}

STDMETHODIMP OpenLessEditSession::DoEditSession(TfEditCookie edit_cookie) {
  const HRESULT hr = cancellation_ && cancellation_->load()
                         ? HRESULT_FROM_WIN32(ERROR_CANCELLED)
                         : InsertText(edit_cookie);
  if (async_state_) {
    async_state_->result = hr;
    if (async_state_->event != nullptr) {
      SetEvent(async_state_->event);
    }
  }
  return hr;
}

HRESULT OpenLessEditSession::InsertText(TfEditCookie edit_cookie) {
  if (context_ == nullptr) {
    return E_UNEXPECTED;
  }
  if (cancellation_ && cancellation_->load()) {
    return HRESULT_FROM_WIN32(ERROR_CANCELLED);
  }

  ITfInsertAtSelection* insert_at_selection = nullptr;
  HRESULT hr = context_->QueryInterface(IID_ITfInsertAtSelection,
                                        reinterpret_cast<void**>(
                                            &insert_at_selection));
  if (FAILED(hr)) {
    return hr;
  }

  if (cancellation_ && cancellation_->load()) {
    insert_at_selection->Release();
    return HRESULT_FROM_WIN32(ERROR_CANCELLED);
  }

  // Commit in a single call. Some Chromium-backed text stores surface a
  // full-text QUERYONLY preflight as an edit and then duplicate the real commit.
  ITfRange* committed_range = nullptr;
  hr = insert_at_selection->InsertTextAtSelection(
      edit_cookie, 0, text_.c_str(), static_cast<LONG>(text_.size()),
      &committed_range);
  if (committed_range != nullptr) {
    if (SUCCEEDED(hr)) {
      const HRESULT collapse_hr =
          committed_range->Collapse(edit_cookie, TF_ANCHOR_END);
      if (SUCCEEDED(collapse_hr)) {
        TF_SELECTION selection = {};
        selection.range = committed_range;
        selection.style.ase = TF_AE_END;
        selection.style.fInterimChar = FALSE;
        (void)context_->SetSelection(edit_cookie, 1, &selection);
      }
    }
    committed_range->Release();
  }

  insert_at_selection->Release();
  return hr;
}
