#pragma once

#include <msctf.h>
#include <atomic>
#include <memory>
#include <string>
#include <windows.h>

struct OpenLessAsyncEditState {
  OpenLessAsyncEditState();
  OpenLessAsyncEditState(const OpenLessAsyncEditState &) = delete;
  OpenLessAsyncEditState &operator=(const OpenLessAsyncEditState &) = delete;
  ~OpenLessAsyncEditState();

  bool IsValid() const;

  HANDLE event = nullptr;
  DWORD create_error = ERROR_SUCCESS;
  HRESULT result = E_UNEXPECTED;
};

class OpenLessEditSession final : public ITfEditSession {
public:
  OpenLessEditSession(ITfContext *context, std::wstring text,
                      std::shared_ptr<OpenLessAsyncEditState> async_state = nullptr,
                      std::shared_ptr<std::atomic<bool>> cancellation = nullptr);
  OpenLessEditSession(const OpenLessEditSession &) = delete;
  OpenLessEditSession &operator=(const OpenLessEditSession &) = delete;
  ~OpenLessEditSession();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP DoEditSession(TfEditCookie edit_cookie) override;

private:
  HRESULT InsertText(TfEditCookie edit_cookie);

  LONG ref_count_ = 1;
  ITfContext *context_ = nullptr;
  std::wstring text_;
  std::shared_ptr<OpenLessAsyncEditState> async_state_;
  std::shared_ptr<std::atomic<bool>> cancellation_;
};
