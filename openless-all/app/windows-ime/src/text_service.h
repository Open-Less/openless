#pragma once

#include <msctf.h>
#include <cstdint>
#include <memory>
#include <string>
#include <windows.h>

#include "ipc_client.h"

struct OpenLessAsyncEditState;
struct OpenLessContextReadState;

class OpenLessTextService final : public ITfTextInputProcessorEx {
 public:
  OpenLessTextService();
  OpenLessTextService(const OpenLessTextService&) = delete;
  OpenLessTextService& operator=(const OpenLessTextService&) = delete;
  ~OpenLessTextService();

  STDMETHODIMP QueryInterface(REFIID iid, void** object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;

  STDMETHODIMP Activate(ITfThreadMgr* thread_mgr, TfClientId client_id) override;
  STDMETHODIMP Deactivate() override;
  STDMETHODIMP ActivateEx(ITfThreadMgr* thread_mgr,
                          TfClientId client_id,
                          DWORD flags) override;

  HRESULT SubmitTextFromPipe(const std::wstring& session_id,
                             const std::wstring& text);
  HRESULT ReadContextFromPipe(uint32_t before_chars,
                              uint32_t after_chars,
                              std::wstring* text,
                              uint32_t* cursor_utf16,
                              bool* blocked);

 private:
  HRESULT StartIpcServer();
  void StopIpcServer();
  HRESULT EnsureMessageWindow();
  void DestroyMessageWindow();
  HRESULT CommitTextOnOwnerThread(
      const std::wstring& session_id,
      const std::wstring& text,
      std::shared_ptr<OpenLessAsyncEditState>* async_completion,
      bool* wait_for_async_completion);
  HRESULT ReadContextOnOwnerThread(
      uint32_t before_chars,
      uint32_t after_chars,
      std::shared_ptr<OpenLessContextReadState>* state,
      bool* wait_for_async_completion);

  static LRESULT CALLBACK MessageWindowProc(HWND window,
                                            UINT message,
                                            WPARAM wparam,
                                            LPARAM lparam);

  LONG ref_count_ = 1;
  ITfThreadMgr* thread_mgr_ = nullptr;
  TfClientId client_id_ = TF_CLIENTID_NULL;
  DWORD owner_thread_id_ = 0;
  HWND message_window_ = nullptr;
  OpenLessPipeServer pipe_server_;
};
