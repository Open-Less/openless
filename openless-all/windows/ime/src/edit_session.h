#pragma once

#include <cstdint>
#include <msctf.h>
#include <memory>
#include <string>
#include <windows.h>

// Shared between the text service and an async edit session. Both only touch
// it on the TSF owner thread, so plain fields are enough.
struct OpenLessAsyncEditState {
  bool cancelled = false;
  bool started = false;
  bool completed = false;
  HRESULT result = E_PENDING;
};

HRESULT ReadOpenLessRangeText(TfEditCookie edit_cookie, ITfRange *range, std::wstring *text);

// Owner-thread state of the live composition that shows the ASR transcript
// while the user is still speaking. TSF can end the composition at any time
// (the user clicks elsewhere), so `terminated` tells the next operation that
// the text already in the document is no longer ours to replace.
struct OpenLessCompositionState {
  ITfComposition *composition = nullptr;
  ITfContext *context = nullptr;
  // A composition was opened for the current dictation.
  bool started = false;
  // TSF ended the composition before the dictation finished.
  bool terminated = false;
  // Range captured when TSF terminated the composition externally. It lets the
  // final edit replace the raw partial instead of appending a duplicate.
  ITfRange *terminated_range = nullptr;
  // Text in the retained range when TSF terminated the composition. Finalization must not
  // overwrite the range if the user changed it after termination.
  std::wstring terminated_text;
  bool terminated_text_captured = false;
  // Text covered by the selection when the composition was first primed. Cancel restores it.
  std::wstring original_text;
  bool original_text_captured = false;
  // Stable owner supplied by the Rust insertion session. A later dictation must not reuse this
  // state while the old composition is still live.
  uint32_t owner_token = 0;
  // Sender process instance for restart recovery. Same-process owner mismatches remain conflicts.
  uint32_t owner_process_id = 0;

  // Drops our references without ending the composition; TSF owns its cleanup.
  void Reset();
};

// What one edit session does to the document.
enum class OpenLessEditOp {
  // Insert the text at the selection; the caller's usual dictation commit.
  kCommit,
  // Replace the live composition with the transcript so far, starting one if needed.
  kUpdate,
  // Replace the live composition with the final text and end it.
  kFinish,
  // Replace the live composition with the original selection and end it without committing.
  kCancel,
};

class OpenLessEditSession final : public ITfEditSession {
public:
  OpenLessEditSession(ITfContext *context, std::wstring text, OpenLessEditOp op,
                      uint32_t composition_owner, uint32_t composition_process_id,
                      std::shared_ptr<OpenLessCompositionState> composition,
                      ITfCompositionSink *composition_sink,
                      std::shared_ptr<OpenLessAsyncEditState> async_state = nullptr);
  OpenLessEditSession(const OpenLessEditSession &) = delete;
  OpenLessEditSession &operator=(const OpenLessEditSession &) = delete;
  ~OpenLessEditSession();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP DoEditSession(TfEditCookie edit_cookie) override;

private:
  HRESULT InsertText(TfEditCookie edit_cookie);
  HRESULT UpdateComposition(TfEditCookie edit_cookie);
  HRESULT FinishComposition(TfEditCookie edit_cookie);
  HRESULT CancelComposition(TfEditCookie edit_cookie);
  HRESULT GetSelectionRange(TfEditCookie edit_cookie, ITfRange **range);

  LONG ref_count_ = 1;
  ITfContext *context_ = nullptr;
  std::wstring text_;
  OpenLessEditOp op_ = OpenLessEditOp::kCommit;
  uint32_t composition_owner_ = 0;
  uint32_t composition_process_id_ = 0;
  std::shared_ptr<OpenLessCompositionState> composition_;
  ITfCompositionSink *composition_sink_ = nullptr;
  std::shared_ptr<OpenLessAsyncEditState> async_state_;
};
