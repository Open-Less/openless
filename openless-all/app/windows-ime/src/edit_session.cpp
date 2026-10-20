#include "edit_session.h"

#include <utility>

#include "display_attribute.h"

extern LONG g_object_count;

namespace {
constexpr HRESULT kStatusCompositionConflict = static_cast<HRESULT>(0x4F4C0007);
}

HRESULT ReadOpenLessRangeText(TfEditCookie edit_cookie, ITfRange *range, std::wstring *text) {
  if (range == nullptr || text == nullptr) {
    return E_INVALIDARG;
  }

  LONG range_start = 0;
  LONG range_length = 0;
  ITfRangeACP *range_acp = nullptr;
  HRESULT hr = range->QueryInterface(IID_ITfRangeACP,
                                     reinterpret_cast<void **>(&range_acp));
  if (FAILED(hr)) {
    return hr;
  }
  hr = range_acp->GetExtent(&range_start, &range_length);
  range_acp->Release();
  if (FAILED(hr)) {
    return hr;
  }
  UNREFERENCED_PARAMETER(range_start);
  if (range_length < 0 || range_length > 16 * 1024 * 1024) {
    return E_INVALIDARG;
  }

  text->assign(static_cast<size_t>(range_length), L'\0');
  if (range_length == 0) {
    return S_OK;
  }

  ULONG copied = 0;
  hr = range->GetText(edit_cookie, 0, text->data(), static_cast<ULONG>(range_length), &copied);
  if (FAILED(hr)) {
    text->clear();
    return hr;
  }
  text->resize(copied);
  return S_OK;
}

void OpenLessCompositionState::Reset() {
  if (composition != nullptr) {
    composition->Release();
    composition = nullptr;
  }
  if (context != nullptr) {
    context->Release();
    context = nullptr;
  }
  if (terminated_range != nullptr) {
    terminated_range->Release();
    terminated_range = nullptr;
  }
  started = false;
  terminated = false;
  terminated_text.clear();
  terminated_text_captured = false;
  original_text.clear();
  original_text_captured = false;
  owner_token = 0;
  owner_process_id = 0;
}

OpenLessEditSession::OpenLessEditSession(ITfContext *context, std::wstring text,
                                         OpenLessEditOp op, uint32_t composition_owner,
                                         uint32_t composition_process_id,
                                         std::shared_ptr<OpenLessCompositionState> composition,
                                         ITfCompositionSink *composition_sink,
                                         std::shared_ptr<OpenLessAsyncEditState> async_state)
    : context_(context), text_(std::move(text)), op_(op),
      composition_owner_(composition_owner), composition_process_id_(composition_process_id),
      composition_(std::move(composition)),
      composition_sink_(composition_sink), async_state_(std::move(async_state)) {
  InterlockedIncrement(&g_object_count);
  if (context_ != nullptr) {
    context_->AddRef();
  }
  if (composition_sink_ != nullptr) {
    composition_sink_->AddRef();
  }
}

OpenLessEditSession::~OpenLessEditSession() {
  if (composition_sink_ != nullptr) {
    composition_sink_->Release();
    composition_sink_ = nullptr;
  }
  if (context_ != nullptr) {
    context_->Release();
    context_ = nullptr;
  }
  InterlockedDecrement(&g_object_count);
}

STDMETHODIMP OpenLessEditSession::QueryInterface(REFIID iid, void **object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  if (iid == IID_IUnknown || iid == IID_ITfEditSession) {
    *object = static_cast<ITfEditSession *>(this);
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
  HRESULT hr = S_OK;
  if (async_state_) {
    async_state_->started = true;
  }
  if (async_state_ && async_state_->cancelled) {
    hr = HRESULT_FROM_WIN32(ERROR_CANCELLED);
  } else {
    switch (op_) {
    case OpenLessEditOp::kCommit:
      hr = InsertText(edit_cookie);
      break;
    case OpenLessEditOp::kUpdate:
      hr = UpdateComposition(edit_cookie);
      break;
    case OpenLessEditOp::kFinish:
      hr = FinishComposition(edit_cookie);
      break;
    case OpenLessEditOp::kCancel:
      hr = CancelComposition(edit_cookie);
      break;
    }
  }
  if (async_state_) {
    async_state_->result = hr;
    async_state_->completed = true;
  }
  return hr;
}

HRESULT OpenLessEditSession::InsertText(TfEditCookie edit_cookie) {
  if (context_ == nullptr) {
    return E_UNEXPECTED;
  }

  ITfInsertAtSelection *insert_at_selection = nullptr;
  HRESULT hr = context_->QueryInterface(IID_ITfInsertAtSelection,
                                        reinterpret_cast<void **>(&insert_at_selection));
  if (FAILED(hr)) {
    return hr;
  }

  // Commit in a single call. Some Chromium-backed text stores surface a
  // full-text QUERYONLY preflight as an edit and then duplicate the real commit.
  ITfRange *committed_range = nullptr;
  hr = insert_at_selection->InsertTextAtSelection(
      edit_cookie, 0, text_.c_str(), static_cast<LONG>(text_.size()), &committed_range);
  if (committed_range != nullptr) {
    if (SUCCEEDED(hr)) {
      const HRESULT collapse_hr = committed_range->Collapse(edit_cookie, TF_ANCHOR_END);
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

HRESULT OpenLessEditSession::GetSelectionRange(TfEditCookie edit_cookie, ITfRange **range) {
  *range = nullptr;
  if (context_ == nullptr) {
    return E_UNEXPECTED;
  }

  ITfInsertAtSelection *insert_at_selection = nullptr;
  HRESULT hr = context_->QueryInterface(IID_ITfInsertAtSelection,
                                        reinterpret_cast<void **>(&insert_at_selection));
  if (FAILED(hr)) {
    return hr;
  }

  // Ask the host text store for the insertion range without changing its contents. This is more
  // reliable than using the raw selection for controls that normalize or virtualize selections.
  hr = insert_at_selection->InsertTextAtSelection(edit_cookie, TF_IAS_QUERYONLY, nullptr, 0, range);
  insert_at_selection->Release();
  if (SUCCEEDED(hr) && *range == nullptr) {
    return E_FAIL;
  }
  return hr;
}

HRESULT OpenLessEditSession::UpdateComposition(TfEditCookie edit_cookie) {
  if (!composition_) {
    return E_UNEXPECTED;
  }
  // Once TSF has ended our composition, the raw text it left behind is
  // already in the document. Starting a new one would stack a second
  // transcript after it, so refuse and let the caller stop streaming.
  if (composition_->terminated) {
    return HRESULT_FROM_WIN32(ERROR_OPERATION_ABORTED);
  }

  if (composition_->composition == nullptr) {
    if (context_ == nullptr || composition_sink_ == nullptr) {
      return E_UNEXPECTED;
    }
    ITfContextComposition *context_composition = nullptr;
    HRESULT hr = context_->QueryInterface(IID_ITfContextComposition,
                                          reinterpret_cast<void **>(&context_composition));
    if (FAILED(hr)) {
      return hr;
    }

    ITfRange *selection_range = nullptr;
    hr = GetSelectionRange(edit_cookie, &selection_range);
    if (FAILED(hr)) {
      context_composition->Release();
      return hr;
    }

    std::wstring original_text;
    hr = ReadOpenLessRangeText(edit_cookie, selection_range, &original_text);
    if (FAILED(hr)) {
      selection_range->Release();
      context_composition->Release();
      return hr;
    }
    composition_->original_text = std::move(original_text);
    composition_->original_text_captured = false;

    ITfComposition *composition = nullptr;
    hr = context_composition->StartComposition(edit_cookie, selection_range, composition_sink_,
                                               &composition);
    selection_range->Release();
    context_composition->Release();
    if (FAILED(hr) || composition == nullptr) {
      return FAILED(hr) ? hr : E_FAIL;
    }

    composition_->composition = composition;
    composition_->context = context_;
    composition_->context->AddRef();
    composition_->started = true;
    composition_->owner_token = composition_owner_;
    composition_->owner_process_id = composition_process_id_;
    composition_->original_text_captured = true;
  }

  ITfRange *range = nullptr;
  HRESULT hr = composition_->composition->GetRange(&range);
  if (FAILED(hr)) {
    return hr;
  }
  hr = range->SetText(edit_cookie, 0, text_.c_str(), static_cast<LONG>(text_.size()));
  if (SUCCEEDED(hr)) {
    ApplyOpenLessInputDisplayAttribute(edit_cookie, context_, range);
    hr = range->Collapse(edit_cookie, TF_ANCHOR_END);
    if (SUCCEEDED(hr)) {
      TF_SELECTION selection = {};
      selection.range = range;
      selection.style.ase = TF_AE_END;
      selection.style.fInterimChar = FALSE;
      hr = context_->SetSelection(edit_cookie, 1, &selection);
    }
  }
  range->Release();
  return hr;
}

HRESULT OpenLessEditSession::CancelComposition(TfEditCookie edit_cookie) {
  if (!composition_) {
    return E_UNEXPECTED;
  }
  if (!composition_->started) {
    composition_->Reset();
    return S_OK;
  }
  if (!composition_->original_text_captured) {
    return E_UNEXPECTED;
  }

  ITfRange *range = nullptr;
  const bool active = composition_->composition != nullptr;
  HRESULT hr = S_OK;
  if (active) {
    hr = composition_->composition->GetRange(&range);
  } else if (composition_->terminated && composition_->terminated_range != nullptr) {
    range = composition_->terminated_range;
    range->AddRef();
  } else {
    // TSF already ended the composition but did not leave a usable range. There is no safe
    // document edit to reconcile; release the stale native ownership so a later cleanup attempt
    // can observe a fresh state instead of retrying this permanently latched one.
    composition_->Reset();
    return HRESULT_FROM_WIN32(ERROR_OPERATION_ABORTED);
  }
  if (FAILED(hr)) {
    return hr;
  }
  if (!active) {
    if (!composition_->terminated_text_captured) {
      range->Release();
      // The range contents are unknown, so preserve them rather than restoring over them.
      composition_->Reset();
      return S_OK;
    }
    std::wstring current_text;
    hr = ReadOpenLessRangeText(edit_cookie, range, &current_text);
    if (FAILED(hr)) {
      range->Release();
      return hr;
    }
    if (current_text != composition_->terminated_text) {
      // The user edited the range after TSF terminated the composition. Preserve their edit
      // instead of restoring the original selection over it.
      range->Release();
      composition_->Reset();
      return S_OK;
    }
  }

  hr = range->SetText(edit_cookie, 0, composition_->original_text.c_str(),
                      static_cast<LONG>(composition_->original_text.size()));
  if (SUCCEEDED(hr)) {
    // Appearance failures must not prevent native composition cleanup.
    (void)ClearOpenLessInputDisplayAttribute(edit_cookie, context_, range);
  }
  bool composition_ended = !active;
  if (SUCCEEDED(hr) && active) {
    hr = composition_->composition->EndComposition(edit_cookie);
    composition_ended = SUCCEEDED(hr);
  }
  HRESULT selection_hr = S_OK;
  if (SUCCEEDED(hr)) {
    TF_SELECTION selection = {};
    selection.range = range;
    selection.style.ase = TF_AE_NONE;
    selection.style.fInterimChar = FALSE;
    selection_hr = context_->SetSelection(edit_cookie, 1, &selection);
  }
  range->Release();
  // EndComposition has already released the native composition even when parking the caret fails.
  // Do not retain its pointer as live ownership and make later cleanup retry a dead composition.
  if (composition_ended && SUCCEEDED(hr)) {
    composition_->Reset();
  }
  return FAILED(hr) ? hr : selection_hr;
}

HRESULT OpenLessEditSession::FinishComposition(TfEditCookie edit_cookie) {
  if (!composition_) {
    return E_UNEXPECTED;
  }
  if (composition_->composition == nullptr) {
    if (composition_->terminated && composition_->terminated_range != nullptr) {
      if (!composition_->terminated_text_captured) {
        // The final text cannot safely replace a range whose ownership snapshot could not be read.
        // Keep the retained range for an explicit Cancel cleanup and report a composition conflict;
        // returning success here would make the caller believe the final text was committed.
        return kStatusCompositionConflict;
      }
      std::wstring current_text;
      HRESULT hr = ReadOpenLessRangeText(edit_cookie, composition_->terminated_range, &current_text);
      if (FAILED(hr)) {
        return hr;
      }
      if (current_text != composition_->terminated_text) {
        // TSF ended the composition and the user changed the retained range. Preserve their edit;
        // the final transcript was not committed, so report a conflict instead of false success.
        return kStatusCompositionConflict;
      }
      hr = composition_->terminated_range->SetText(edit_cookie, 0, text_.c_str(),
                                                   static_cast<LONG>(text_.size()));
      if (SUCCEEDED(hr)) {
        (void)ClearOpenLessInputDisplayAttribute(edit_cookie, context_,
                                                composition_->terminated_range);
      }
      if (SUCCEEDED(hr) &&
          SUCCEEDED(composition_->terminated_range->Collapse(edit_cookie, TF_ANCHOR_END))) {
        TF_SELECTION selection = {};
        selection.range = composition_->terminated_range;
        selection.style.ase = TF_AE_END;
        selection.style.fInterimChar = FALSE;
        (void)context_->SetSelection(edit_cookie, 1, &selection);
      }
      if (SUCCEEDED(hr)) {
        composition_->Reset();
      }
      return hr;
    }
    // Terminated without a usable range: inserting the final text would either
    // duplicate the raw partial or target the wrong document.
    if (composition_->started || composition_->terminated) {
      return HRESULT_FROM_WIN32(ERROR_OPERATION_ABORTED);
    }
    // Nothing was ever composed for this dictation: a plain commit.
    return InsertText(edit_cookie);
  }

  ITfRange *range = nullptr;
  HRESULT hr = composition_->composition->GetRange(&range);
  if (FAILED(hr)) {
    return hr;
  }
  hr = range->SetText(edit_cookie, 0, text_.c_str(), static_cast<LONG>(text_.size()));
  if (SUCCEEDED(hr)) {
    (void)ClearOpenLessInputDisplayAttribute(edit_cookie, context_, range);
    hr = composition_->composition->EndComposition(edit_cookie);
  }
  if (SUCCEEDED(hr)) {
    // Park the caret after the committed text.
    if (SUCCEEDED(range->Collapse(edit_cookie, TF_ANCHOR_END))) {
      TF_SELECTION selection = {};
      selection.range = range;
      selection.style.ase = TF_AE_END;
      selection.style.fInterimChar = FALSE;
      (void)context_->SetSelection(edit_cookie, 1, &selection);
    }
  }
  range->Release();
  if (SUCCEEDED(hr)) {
    composition_->Reset();
  }
  return hr;
}
