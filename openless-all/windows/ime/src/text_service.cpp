#include "text_service.h"

#include <cstring>
#include <new>
#include <utility>

#include "display_attribute.h"
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
constexpr ULONG_PTR kCopyDataSubmitLegacy = 0x4F4C5331; // "OLS1": uint32 token + UTF-16LE text
constexpr ULONG_PTR kCopyDataSubmit = 0x4F4C5332; // "OLS2": token + sender process + focus HWND + UTF-16LE text
constexpr ULONG_PTR kCopyDataQuery = 0x4F4C5131;  // "OLQ1": uint32 token
constexpr ULONG_PTR kCopyDataUpdate = 0x4F4C5531; // "OLU1": token + owner + sender process + focus HWND + UTF-16LE text, replace the composition
constexpr ULONG_PTR kCopyDataFinish = 0x4F4C4631; // "OLF1": token + owner + sender process + focus HWND + UTF-16LE text, end the composition
constexpr ULONG_PTR kCopyDataCancel = 0x4F4C4331; // "OLC1": token + owner + sender process + focus HWND + UTF-16LE text, restore selection
constexpr ULONG_PTR kCopyDataVersion = 0x4F4C5631; // "OLV1": zero-length protocol probe
constexpr DWORD kMaxSubmitBytes = 1024 * 1024;

// Replies are nonzero so they differ from an unhandled message (0). A failed
// commit is reported as its HRESULT, which always has the high bit set.
constexpr LRESULT kStatusAccepted = 0x4F4C0001;
constexpr LRESULT kStatusPending = 0x4F4C0002;
constexpr LRESULT kStatusCommitted = 0x4F4C0003;
constexpr LRESULT kStatusUnknownToken = 0x4F4C0004;
constexpr LRESULT kStatusBadRequest = 0x4F4C0005;
constexpr LRESULT kStatusNotDispatched = 0x4F4C0006;
constexpr LRESULT kStatusCompositionConflict = 0x4F4C0007;
constexpr LRESULT kStatusCompositionRecoveryRequired = 0x4F4C0008;
constexpr LRESULT kStatusProtocolVersion = 0x4F4C0009;

LRESULT StatusFromHResult(HRESULT hr, bool dispatched) {
  if (hr == static_cast<HRESULT>(kStatusCompositionConflict)) {
    return kStatusCompositionConflict;
  }
  if (hr == static_cast<HRESULT>(kStatusCompositionRecoveryRequired)) {
    return kStatusCompositionRecoveryRequired;
  }
  if (SUCCEEDED(hr)) {
    return kStatusCommitted;
  }
  if (!dispatched) {
    return kStatusNotDispatched;
  }
  return static_cast<LRESULT>(hr);
}

bool ReadToken(const COPYDATASTRUCT *copy_data, uint32_t *token) {
  if (copy_data->lpData == nullptr || copy_data->cbData < sizeof(uint32_t)) {
    return false;
  }
  std::memcpy(token, copy_data->lpData, sizeof(uint32_t));
  return *token != 0;
}

bool TargetWindowIsFocused(uintptr_t target_window) {
  const HWND target = reinterpret_cast<HWND>(target_window);
  const DWORD target_thread = GetWindowThreadProcessId(target, nullptr);
  if (target_thread == 0) {
    return false;
  }
  GUITHREADINFO gui_info = {};
  gui_info.cbSize = sizeof(gui_info);
  if (!GetGUIThreadInfo(target_thread, &gui_info)) {
    return false;
  }
  return reinterpret_cast<ULONG_PTR>(gui_info.hwndFocus) == target_window;
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
  if (iid == IID_ITfCompositionSink) {
    *object = static_cast<ITfCompositionSink *>(this);
    AddRef();
    return S_OK;
  }
  if (iid == IID_ITfDisplayAttributeProvider) {
    *object = static_cast<ITfDisplayAttributeProvider *>(this);
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
  // Only drop our references. Ending the composition here would need an edit
  // session, and TSF cleans the composition up when the service goes away.
  if (composition_) {
    composition_->Reset();
  }

  if (thread_mgr_ != nullptr) {
    thread_mgr_->Release();
    thread_mgr_ = nullptr;
  }
  client_id_ = TF_CLIENTID_NULL;

  return S_OK;
}

STDMETHODIMP OpenLessTextService::OnCompositionTerminated(TfEditCookie edit_cookie,
                                                          ITfComposition *composition) {
  if (composition_ && composition_->composition == composition) {
    if (composition_->terminated_range != nullptr) {
      composition_->terminated_range->Release();
      composition_->terminated_range = nullptr;
    }
    composition_->terminated_text.clear();
    composition_->terminated_text_captured = false;
    // Keep the range and context alive so the final edit can replace the raw
    // partial after TSF terminates the composition externally. Snapshot the
    // text as an ownership check: a later user edit must not be overwritten.
    if (SUCCEEDED(composition->GetRange(&composition_->terminated_range)) &&
        composition_->terminated_range != nullptr) {
      (void)ClearOpenLessInputDisplayAttribute(edit_cookie, composition_->context,
                                              composition_->terminated_range);
      if (SUCCEEDED(ReadOpenLessRangeText(edit_cookie, composition_->terminated_range,
                                          &composition_->terminated_text))) {
        composition_->terminated_text_captured = true;
      }
    }
    composition_->composition->Release();
    composition_->composition = nullptr;
    composition_->terminated = true;
  }
  return S_OK;
}

STDMETHODIMP OpenLessTextService::EnumDisplayAttributeInfo(
    IEnumTfDisplayAttributeInfo **enum_info) {
  if (enum_info == nullptr) {
    return E_INVALIDARG;
  }
  auto *enumerator = new (std::nothrow) OpenLessEnumDisplayAttributeInfo();
  if (enumerator == nullptr) {
    return E_OUTOFMEMORY;
  }
  *enum_info = enumerator;
  return S_OK;
}

STDMETHODIMP OpenLessTextService::GetDisplayAttributeInfo(REFGUID guid,
                                                          ITfDisplayAttributeInfo **info) {
  if (info == nullptr) {
    return E_INVALIDARG;
  }
  *info = nullptr;
  if (!IsEqualGUID(guid, GUID_OpenLessDisplayAttribute)) {
    return E_INVALIDARG;
  }
  auto *attribute = new (std::nothrow) OpenLessDisplayAttributeInfo();
  if (attribute == nullptr) {
    return E_OUTOFMEMORY;
  }
  *info = attribute;
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
  if (copy_data->dwData == kCopyDataVersion) {
    return copy_data->cbData == 0 ? kStatusProtocolVersion : kStatusBadRequest;
  }
  if (copy_data->dwData == kCopyDataSubmitLegacy) {
    return AcceptSubmit(copy_data, OpenLessEditOp::kCommit, true);
  }
  if (copy_data->dwData == kCopyDataSubmit) {
    return AcceptSubmit(copy_data, OpenLessEditOp::kCommit);
  }
  if (copy_data->dwData == kCopyDataUpdate) {
    return AcceptSubmit(copy_data, OpenLessEditOp::kUpdate);
  }
  if (copy_data->dwData == kCopyDataFinish) {
    return AcceptSubmit(copy_data, OpenLessEditOp::kFinish);
  }
  if (copy_data->dwData == kCopyDataCancel) {
    return AcceptSubmit(copy_data, OpenLessEditOp::kCancel);
  }
  if (copy_data->dwData == kCopyDataQuery) {
    return QuerySubmit(copy_data);
  }
  return kStatusBadRequest;
}

LRESULT OpenLessTextService::AcceptSubmit(const COPYDATASTRUCT *copy_data, OpenLessEditOp op,
                                           bool legacy_commit_payload) {
  uint32_t token = 0;
  if (!ReadToken(copy_data, &token) || copy_data->cbData > kMaxSubmitBytes) {
    return kStatusBadRequest;
  }

  size_t text_offset = sizeof(uint32_t);
  uint32_t composition_owner = 0;
  uint32_t composition_process_id = 0;
  uintptr_t target_window = 0;
  if (op == OpenLessEditOp::kCommit && legacy_commit_payload) {
    // Keep accepting requests from an older OpenLess process. Legacy commits cannot participate in
    // restart recovery because they do not identify their sender process.
    text_offset = sizeof(uint32_t);
  } else if (op == OpenLessEditOp::kCommit) {
    constexpr size_t kCommitHeaderBytes = sizeof(uint32_t) * 2 + sizeof(uint64_t);
    if (copy_data->cbData < kCommitHeaderBytes) {
      return kStatusBadRequest;
    }
    std::memcpy(&composition_process_id,
                static_cast<const BYTE *>(copy_data->lpData) + sizeof(uint32_t),
                sizeof(composition_process_id));
    std::uint64_t target_window_value = 0;
    std::memcpy(&target_window_value,
                static_cast<const BYTE *>(copy_data->lpData) + sizeof(uint32_t) * 2,
                sizeof(target_window_value));
    target_window = static_cast<uintptr_t>(target_window_value);
    if (composition_process_id == 0) {
      return kStatusBadRequest;
    }
    text_offset = kCommitHeaderBytes;
  } else {
    constexpr size_t kCompositionHeaderBytes = sizeof(uint32_t) * 3 + sizeof(uint64_t);
    if (copy_data->cbData < kCompositionHeaderBytes) {
      return kStatusBadRequest;
    }
    std::memcpy(&composition_owner,
                static_cast<const BYTE *>(copy_data->lpData) + sizeof(uint32_t),
                sizeof(composition_owner));
    std::memcpy(&composition_process_id,
                static_cast<const BYTE *>(copy_data->lpData) + sizeof(uint32_t) * 2,
                sizeof(composition_process_id));
    std::uint64_t target_window_value = 0;
    std::memcpy(&target_window_value,
                static_cast<const BYTE *>(copy_data->lpData) + sizeof(uint32_t) * 3,
                sizeof(target_window_value));
    target_window = static_cast<uintptr_t>(target_window_value);
    if (composition_owner == 0 || composition_process_id == 0) {
      return kStatusBadRequest;
    }
    text_offset = kCompositionHeaderBytes;
  }
  if (copy_data->cbData < text_offset ||
      (copy_data->cbData - text_offset) % sizeof(wchar_t) != 0) {
    return kStatusBadRequest;
  }

  CancelPendingSubmit();

  const size_t text_bytes = copy_data->cbData - text_offset;
  submit_text_.assign(text_bytes / sizeof(wchar_t), L'\0');
  std::memcpy(submit_text_.data(), static_cast<const BYTE *>(copy_data->lpData) + text_offset,
              text_bytes);
  submit_token_ = token;
  submit_composition_owner_ = composition_owner;
  submit_composition_process_id_ = composition_process_id;
  submit_target_window_ = target_window;
  submit_op_ = op;
  submit_status_ = kStatusPending;

  // The sender's SendMessage can be dispatched while the host is in the middle
  // of something else. Commit from a posted message instead, so the edit
  // session is requested from the top of the host message loop.
  if (!PostMessageW(message_window_, kRunSubmitMessage, token, 0)) {
    const DWORD error = GetLastError();
    CancelPendingSubmit();
    return StatusFromHResult(
        HRESULT_FROM_WIN32(error != ERROR_SUCCESS ? error : ERROR_GEN_FAILURE), true);
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
    const bool native_composition_owned =
        composition_ && (composition_->composition != nullptr || composition_->started ||
                         composition_->terminated || composition_->owner_token != 0);
    const bool update_failed_before_composition =
        submit_op_ == OpenLessEditOp::kUpdate && FAILED(async_edit_->result) && composition_ &&
        !native_composition_owned;
    bool dispatched = async_edit_->started;
    if (FAILED(async_edit_->result) && native_composition_owned) {
      // A live composition remains unsafe to fall back beside even when TSF rejects the edit
      // before invoking DoEditSession.
      dispatched = true;
    }
    submit_status_ = StatusFromHResult(async_edit_->result,
                                       update_failed_before_composition ? false : dispatched);
    async_edit_.reset();
  }
  return submit_status_;
}

void OpenLessTextService::RunPendingSubmit(uint32_t token) {
  if (token != submit_token_ || submit_status_ != kStatusPending || async_edit_) {
    return; // Superseded, cancelled, or already running.
  }

  std::shared_ptr<OpenLessAsyncEditState> async_edit;
  bool dispatched = false;
  HRESULT hr = E_UNEXPECTED;
  try {
    const std::wstring text = std::move(submit_text_);
    submit_text_.clear();
    hr = CommitTextOnOwnerThread(text, submit_op_, submit_composition_owner_,
                                 submit_composition_process_id_, submit_target_window_, &async_edit,
                                 &dispatched);
  } catch (const std::bad_alloc &) {
    hr = E_OUTOFMEMORY;
  } catch (...) {
    hr = E_UNEXPECTED;
  }

  const bool native_composition_owned =
      composition_ && (composition_->composition != nullptr || composition_->started ||
                       composition_->terminated || composition_->owner_token != 0);
  if (FAILED(hr) && native_composition_owned) {
    // A live composition remains unsafe to fall back beside even when TSF rejects the edit before
    // invoking DoEditSession.
    dispatched = true;
  }
  if (submit_op_ == OpenLessEditOp::kUpdate && FAILED(hr) && composition_ &&
      !native_composition_owned) {
    // A dispatched edit session can still fail before StartComposition. Such a failure cannot
    // leave native composition state, so it must remain eligible for the configured fallback.
    dispatched = false;
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
  submit_status_ = StatusFromHResult(hr, dispatched);
}

void OpenLessTextService::CancelPendingSubmit() {
  if (async_edit_) {
    async_edit_->cancelled = true;
    async_edit_.reset();
  }
  submit_text_.clear();
  submit_token_ = 0;
  submit_composition_owner_ = 0;
  submit_composition_process_id_ = 0;
  submit_target_window_ = 0;
  submit_status_ = 0;
  submit_op_ = OpenLessEditOp::kCommit;
}

HRESULT OpenLessTextService::CommitTextOnOwnerThread(
    const std::wstring &text, OpenLessEditOp op, uint32_t composition_owner,
    uint32_t composition_process_id, uintptr_t target_window,
    std::shared_ptr<OpenLessAsyncEditState> *async_edit,
    bool *dispatched) {
  *async_edit = nullptr;
  *dispatched = false;
  if (thread_mgr_ == nullptr || client_id_ == TF_CLIENTID_NULL) {
    return E_UNEXPECTED;
  }

  if (op == OpenLessEditOp::kCommit) {
    if (composition_ &&
        (composition_->composition != nullptr || composition_->started ||
         composition_->terminated || composition_->owner_token != 0)) {
      const bool restart_recovery =
          composition_->owner_process_id != 0 && composition_process_id != 0 &&
          composition_->owner_process_id != composition_process_id;
      if (restart_recovery) {
        // Rust will cancel the old process's context first, then retry the plain commit. Keep
        // recovery classification even when the captured target is no longer focused; returning a
        // generic E_ABORT would let the caller fall back beside the still-owned composition.
        *dispatched = true;
        return static_cast<HRESULT>(kStatusCompositionRecoveryRequired);
      }
      // The native composition is live or its raw range is still recoverable, so callers must not
      // treat this rejection as safe pre-dispatch fallback.
      *dispatched = true;
      return static_cast<HRESULT>(kStatusCompositionConflict);
    }
  } else {
    if (composition_owner == 0) {
      return E_INVALIDARG;
    }
    if (op != OpenLessEditOp::kUpdate && composition_ && composition_->owner_token == 0) {
      if ((op == OpenLessEditOp::kCancel || op == OpenLessEditOp::kFinish) &&
          !composition_->started && !composition_->terminated &&
          composition_->composition == nullptr) {
        // An accepted async update may be canceled or finished before StartComposition. There is
        // no native range to clean, so report a definite pre-dispatch result for fallback.
        *dispatched = false;
        return E_ABORT;
      }
      // Finish/Cancel may not fall through to InsertText when the prime never opened a composition.
      *dispatched = true;
      return static_cast<HRESULT>(kStatusCompositionConflict);
    }
    const bool owner_mismatch = composition_ && composition_->owner_token != 0 &&
                                composition_->owner_token != composition_owner;
    const bool restart_recovery =
        op == OpenLessEditOp::kUpdate && owner_mismatch && composition_->owner_process_id != 0 &&
        composition_->owner_process_id != composition_process_id;
    if (restart_recovery) {
      // Rust will cancel the old process's context first. The subsequent update/commit validates
      // the current focus independently; returning recovery here prevents unsafe fallback while
      // the orphaned native composition is still live.
      *dispatched = true;
      return static_cast<HRESULT>(kStatusCompositionRecoveryRequired);
    }
    if (owner_mismatch &&
        !(op == OpenLessEditOp::kCancel && composition_->owner_process_id != 0 &&
          composition_->owner_process_id != composition_process_id)) {
      // A different insertion in the same OpenLess process still owns the live composition. A
      // changed process ID may issue only the recovery Cancel handled by the old context.
      *dispatched = true;
      return static_cast<HRESULT>(kStatusCompositionConflict);
    }
  }

  if (op == OpenLessEditOp::kUpdate && composition_ &&
      composition_->composition == nullptr && !composition_->terminated &&
      composition_->owner_token == 0 && target_window != 0 &&
      !TargetWindowIsFocused(target_window)) {
    // The focus changed after Rust captured and reactivated the insertion target. Do not prime a
    // composition in the new control; the caller must keep the original target or fall back.
    *dispatched = false;
    return E_ABORT;
  }

  // A plain commit starts a dictation that never composed text. Drop any leftover state from
  // an earlier composition that the host ended, so it cannot reject this dictation's updates.
  // A live composition is left alone: the host still owns it until its final edit.
  if (op == OpenLessEditOp::kCommit && composition_ && composition_->composition == nullptr) {
    composition_->Reset();
  }

  // Updates and the final replacement must land in the context that owns the
  // composition, even if focus has moved since it was started.
  ITfContext *context = nullptr;
  if (op != OpenLessEditOp::kCommit && composition_ && composition_->context != nullptr) {
    context = composition_->context;
    context->AddRef();
  } else {
    ITfDocumentMgr *document_mgr = nullptr;
    HRESULT hr = thread_mgr_->GetFocus(&document_mgr);
    if (FAILED(hr)) {
      return hr;
    }
    if (document_mgr == nullptr) {
      return E_FAIL;
    }

    hr = document_mgr->GetTop(&context);
    document_mgr->Release();
    document_mgr = nullptr;
    if (FAILED(hr)) {
      return hr;
    }
    if (context == nullptr) {
      return E_FAIL;
    }
  }

  const TfClientId client_id = client_id_;
  auto sync_state = std::make_shared<OpenLessAsyncEditState>();
  auto *session = new (std::nothrow) OpenLessEditSession(
      context, text, op, composition_owner, composition_process_id, composition_,
      static_cast<ITfCompositionSink *>(this),
      sync_state);
  if (session == nullptr) {
    context->Release();
    return E_OUTOFMEMORY;
  }

  HRESULT edit_result = S_OK;
  HRESULT hr = context->RequestEditSession(client_id, session, TF_ES_SYNC | TF_ES_READWRITE,
                                           &edit_result);
  session->Release();

  const bool synchronous_rejected =
      hr == TF_E_SYNCHRONOUS || (SUCCEEDED(hr) && edit_result == TF_E_SYNCHRONOUS);
  if (!synchronous_rejected) {
    *dispatched = sync_state->started;
    context->Release();
    if (FAILED(hr)) {
      return hr;
    }
    return edit_result;
  }

  // Hosts such as Word refuse a synchronous lock; queue the edit instead and
  // let the caller poll for its completion.
  auto completion = std::make_shared<OpenLessAsyncEditState>();
  auto *async_session = new (std::nothrow) OpenLessEditSession(
      context, text, op, composition_owner, composition_process_id, composition_,
      static_cast<ITfCompositionSink *>(this), completion);
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
    *dispatched = completion->started;
    return hr;
  }
  if (FAILED(async_edit_result)) {
    *dispatched = completion->started;
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
    result = StatusFromHResult(E_OUTOFMEMORY, false);
  } catch (...) {
    result = StatusFromHResult(E_UNEXPECTED, false);
  }
  service->Release();
  return result;
}
