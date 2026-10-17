#pragma once

#include <msctf.h>
#include <cstdint>
#include <memory>
#include <string>
#include <windows.h>

#include "edit_session.h"

// The text service owns no threads. OpenLess submits text by sending
// WM_COPYDATA to the message-only window created on the TSF owner thread, so
// Activate/Deactivate never wait on anything inside the host process.
class OpenLessTextService final : public ITfTextInputProcessorEx,
                                  public ITfCompositionSink,
                                  public ITfDisplayAttributeProvider {
public:
  OpenLessTextService();
  OpenLessTextService(const OpenLessTextService &) = delete;
  OpenLessTextService &operator=(const OpenLessTextService &) = delete;
  ~OpenLessTextService();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;

  STDMETHODIMP Activate(ITfThreadMgr *thread_mgr, TfClientId client_id) override;
  STDMETHODIMP Deactivate() override;
  STDMETHODIMP ActivateEx(ITfThreadMgr *thread_mgr, TfClientId client_id, DWORD flags) override;

  // ITfCompositionSink: TSF ended our composition (for example the user clicked away).
  STDMETHODIMP OnCompositionTerminated(TfEditCookie edit_cookie, ITfComposition *composition) override;

  // ITfDisplayAttributeProvider: hosts ask how to paint the live transcript underline.
  STDMETHODIMP EnumDisplayAttributeInfo(IEnumTfDisplayAttributeInfo **enum_info) override;
  STDMETHODIMP GetDisplayAttributeInfo(REFGUID guid, ITfDisplayAttributeInfo **info) override;

private:
  HRESULT EnsureMessageWindow();
  void DestroyMessageWindow();
  LRESULT HandleCopyData(const COPYDATASTRUCT *copy_data);
  LRESULT AcceptSubmit(const COPYDATASTRUCT *copy_data, OpenLessEditOp op,
                       bool legacy_commit_payload = false);
  LRESULT QuerySubmit(const COPYDATASTRUCT *copy_data);
  void RunPendingSubmit(uint32_t token);
  void CancelPendingSubmit();
  HRESULT CommitTextOnOwnerThread(const std::wstring &text, OpenLessEditOp op,
                                  uint32_t composition_owner, uint32_t composition_process_id,
                                  uintptr_t target_window,
                                  std::shared_ptr<OpenLessAsyncEditState> *async_edit,
                                  bool *dispatched);

  static LRESULT CALLBACK MessageWindowProc(HWND window, UINT message, WPARAM wparam,
                                            LPARAM lparam);

  LONG ref_count_ = 1;
  ITfThreadMgr *thread_mgr_ = nullptr;
  TfClientId client_id_ = TF_CLIENTID_NULL;
  HWND message_window_ = nullptr;

  // At most one submit is tracked; a newer one supersedes it. Owner thread only.
  uint32_t submit_token_ = 0;
  uint32_t submit_composition_owner_ = 0;
  uint32_t submit_composition_process_id_ = 0;
  uintptr_t submit_target_window_ = 0;
  LRESULT submit_status_ = 0;
  std::wstring submit_text_;
  OpenLessEditOp submit_op_ = OpenLessEditOp::kCommit;
  std::shared_ptr<OpenLessAsyncEditState> async_edit_;

  // The live composition for the current dictation. Owner thread only.
  std::shared_ptr<OpenLessCompositionState> composition_ = std::make_shared<OpenLessCompositionState>();
};
