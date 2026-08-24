#pragma once

#include <msctf.h>
#include <cstdint>
#include <memory>
#include <string>
#include <windows.h>

struct OpenLessAsyncEditState {
  OpenLessAsyncEditState();
  OpenLessAsyncEditState(const OpenLessAsyncEditState&) = delete;
  OpenLessAsyncEditState& operator=(const OpenLessAsyncEditState&) = delete;
  ~OpenLessAsyncEditState();

  bool IsValid() const;

  HANDLE event = nullptr;
  DWORD create_error = ERROR_SUCCESS;
  HRESULT result = E_UNEXPECTED;
};

struct OpenLessContextReadState {
  OpenLessContextReadState();
  OpenLessContextReadState(const OpenLessContextReadState&) = delete;
  OpenLessContextReadState& operator=(const OpenLessContextReadState&) = delete;
  ~OpenLessContextReadState();

  bool IsValid() const;

  HANDLE event = nullptr;
  DWORD create_error = ERROR_SUCCESS;
  HRESULT result = E_UNEXPECTED;
  std::wstring text;
  uint32_t cursor_utf16 = 0;
  bool blocked = false;
};

class OpenLessEditSession final : public ITfEditSession {
 public:
  OpenLessEditSession(
      ITfContext* context,
      std::wstring text,
      std::shared_ptr<OpenLessAsyncEditState> async_state = nullptr);
  OpenLessEditSession(const OpenLessEditSession&) = delete;
  OpenLessEditSession& operator=(const OpenLessEditSession&) = delete;
  ~OpenLessEditSession();

  STDMETHODIMP QueryInterface(REFIID iid, void** object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP DoEditSession(TfEditCookie edit_cookie) override;

 private:
  HRESULT InsertText(TfEditCookie edit_cookie);

  LONG ref_count_ = 1;
  ITfContext* context_ = nullptr;
  std::wstring text_;
  std::shared_ptr<OpenLessAsyncEditState> async_state_;
};

class OpenLessContextReadSession final : public ITfEditSession {
 public:
  OpenLessContextReadSession(
      ITfContext* context,
      uint32_t before_chars,
      uint32_t after_chars,
      std::shared_ptr<OpenLessContextReadState> state);
  OpenLessContextReadSession(const OpenLessContextReadSession&) = delete;
  OpenLessContextReadSession& operator=(const OpenLessContextReadSession&) = delete;
  ~OpenLessContextReadSession();

  STDMETHODIMP QueryInterface(REFIID iid, void** object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP DoEditSession(TfEditCookie edit_cookie) override;

 private:
  HRESULT ReadContext(TfEditCookie edit_cookie);

  LONG ref_count_ = 1;
  ITfContext* context_ = nullptr;
  uint32_t before_chars_ = 0;
  uint32_t after_chars_ = 0;
  std::shared_ptr<OpenLessContextReadState> state_;
};
