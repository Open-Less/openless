#include "edit_session.h"

#include <algorithm>
#include <inputscope.h>
#include <utility>
#include <vector>

namespace {

constexpr uint32_t kMaxContextCharsPerSide = 2048;

bool IsPasswordScope(ITfContext* context,
                     TfEditCookie edit_cookie,
                     ITfRange* range) {
  ITfProperty* property = nullptr;
  if (FAILED(context->GetProperty(GUID_PROP_INPUTSCOPE, &property)) ||
      property == nullptr) {
    return false;
  }
  VARIANT value;
  VariantInit(&value);
  const HRESULT value_hr = property->GetValue(edit_cookie, range, &value);
  property->Release();
  if (FAILED(value_hr) || value.vt != VT_UNKNOWN || value.punkVal == nullptr) {
    VariantClear(&value);
    return false;
  }

  ITfInputScope* input_scope = nullptr;
  const HRESULT scope_hr = value.punkVal->QueryInterface(
      IID_ITfInputScope, reinterpret_cast<void**>(&input_scope));
  bool blocked = false;
  if (SUCCEEDED(scope_hr) && input_scope != nullptr) {
    InputScope* scopes = nullptr;
    UINT count = 0;
    if (SUCCEEDED(input_scope->GetInputScopes(&scopes, &count))) {
      for (UINT i = 0; i < count; ++i) {
        if (scopes[i] == IS_PASSWORD) {
          blocked = true;
          break;
        }
      }
      CoTaskMemFree(scopes);
    }
    input_scope->Release();
  }
  VariantClear(&value);
  return blocked;
}

HRESULT ReadRangeText(ITfRange* range,
                      TfEditCookie edit_cookie,
                      uint32_t max_chars,
                      std::wstring* output) {
  output->clear();
  if (max_chars == 0) {
    return S_OK;
  }
  const LONG capacity = static_cast<LONG>(
      std::min(max_chars, kMaxContextCharsPerSide));
  std::vector<wchar_t> buffer(static_cast<size_t>(capacity));
  LONG fetched = 0;
  const HRESULT hr = range->GetText(edit_cookie, 0, buffer.data(), capacity,
                                    &fetched);
  if (SUCCEEDED(hr) && fetched > 0) {
    output->assign(buffer.data(), static_cast<size_t>(fetched));
  }
  return hr;
}

}  // namespace

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

OpenLessContextReadState::OpenLessContextReadState()
    : event(CreateEventW(nullptr, TRUE, FALSE, nullptr)) {
  if (event == nullptr) {
    create_error = GetLastError();
  }
}

OpenLessContextReadState::~OpenLessContextReadState() {
  if (event != nullptr) {
    CloseHandle(event);
    event = nullptr;
  }
}

bool OpenLessContextReadState::IsValid() const {
  return event != nullptr;
}

OpenLessEditSession::OpenLessEditSession(
    ITfContext* context,
    std::wstring text,
    std::shared_ptr<OpenLessAsyncEditState> async_state)
    : context_(context),
      text_(std::move(text)),
      async_state_(std::move(async_state)) {
  if (context_ != nullptr) {
    context_->AddRef();
  }
}

OpenLessEditSession::~OpenLessEditSession() {
  if (context_ != nullptr) {
    context_->Release();
    context_ = nullptr;
  }
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
  const HRESULT hr = InsertText(edit_cookie);
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

  ITfInsertAtSelection* insert_at_selection = nullptr;
  HRESULT hr = context_->QueryInterface(IID_ITfInsertAtSelection,
                                        reinterpret_cast<void**>(
                                            &insert_at_selection));
  if (FAILED(hr)) {
    return hr;
  }

  ITfRange* query_range = nullptr;
  hr = insert_at_selection->InsertTextAtSelection(
      edit_cookie, TF_IAS_QUERYONLY, text_.c_str(),
      static_cast<LONG>(text_.size()), &query_range);
  if (query_range != nullptr) {
    query_range->Release();
    query_range = nullptr;
  }

  if (SUCCEEDED(hr)) {
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
  }

  insert_at_selection->Release();
  return hr;
}

OpenLessContextReadSession::OpenLessContextReadSession(
    ITfContext* context,
    uint32_t before_chars,
    uint32_t after_chars,
    std::shared_ptr<OpenLessContextReadState> state)
    : context_(context),
      before_chars_(std::min(before_chars, kMaxContextCharsPerSide)),
      after_chars_(std::min(after_chars, kMaxContextCharsPerSide)),
      state_(std::move(state)) {
  if (context_ != nullptr) {
    context_->AddRef();
  }
}

OpenLessContextReadSession::~OpenLessContextReadSession() {
  if (context_ != nullptr) {
    context_->Release();
    context_ = nullptr;
  }
}

STDMETHODIMP OpenLessContextReadSession::QueryInterface(REFIID iid,
                                                        void** object) {
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

STDMETHODIMP_(ULONG) OpenLessContextReadSession::AddRef() {
  return static_cast<ULONG>(InterlockedIncrement(&ref_count_));
}

STDMETHODIMP_(ULONG) OpenLessContextReadSession::Release() {
  const ULONG count = static_cast<ULONG>(InterlockedDecrement(&ref_count_));
  if (count == 0) {
    delete this;
  }
  return count;
}

STDMETHODIMP OpenLessContextReadSession::DoEditSession(
    TfEditCookie edit_cookie) {
  const HRESULT hr = ReadContext(edit_cookie);
  if (state_) {
    state_->result = hr;
    if (state_->event != nullptr) {
      SetEvent(state_->event);
    }
  }
  return hr;
}

HRESULT OpenLessContextReadSession::ReadContext(TfEditCookie edit_cookie) {
  if (context_ == nullptr || !state_) {
    return E_UNEXPECTED;
  }
  TF_SELECTION selection = {};
  ULONG fetched = 0;
  HRESULT hr = context_->GetSelection(edit_cookie, TF_DEFAULT_SELECTION, 1,
                                      &selection, &fetched);
  if (FAILED(hr) || fetched == 0 || selection.range == nullptr) {
    return FAILED(hr) ? hr : E_FAIL;
  }

  ITfRange* caret = selection.range;
  hr = caret->Collapse(edit_cookie, TF_ANCHOR_END);
  if (FAILED(hr)) {
    caret->Release();
    return hr;
  }
  if (IsPasswordScope(context_, edit_cookie, caret)) {
    state_->blocked = true;
    caret->Release();
    return E_ACCESSDENIED;
  }

  ITfRange* left = nullptr;
  ITfRange* right = nullptr;
  hr = caret->Clone(&left);
  if (SUCCEEDED(hr)) {
    hr = caret->Clone(&right);
  }
  caret->Release();
  if (FAILED(hr) || left == nullptr || right == nullptr) {
    if (left != nullptr) left->Release();
    if (right != nullptr) right->Release();
    return FAILED(hr) ? hr : E_FAIL;
  }

  LONG shifted = 0;
  hr = left->ShiftStart(edit_cookie, -static_cast<LONG>(before_chars_),
                        &shifted, nullptr);
  std::wstring before;
  if (SUCCEEDED(hr)) {
    hr = ReadRangeText(left, edit_cookie, before_chars_, &before);
  }
  left->Release();
  if (FAILED(hr)) {
    right->Release();
    return hr;
  }

  shifted = 0;
  hr = right->ShiftEnd(edit_cookie, static_cast<LONG>(after_chars_),
                       &shifted, nullptr);
  std::wstring after;
  if (SUCCEEDED(hr)) {
    hr = ReadRangeText(right, edit_cookie, after_chars_, &after);
  }
  right->Release();
  if (FAILED(hr)) {
    return hr;
  }

  state_->cursor_utf16 = static_cast<uint32_t>(before.size());
  state_->text = std::move(before);
  state_->text += after;
  return S_OK;
}
