#pragma once

#include <atomic>
#include <string>
#include <thread>
#include <windows.h>

class OpenLessTextService;

class OpenLessPipeServer {
public:
  OpenLessPipeServer();
  OpenLessPipeServer(const OpenLessPipeServer &) = delete;
  OpenLessPipeServer &operator=(const OpenLessPipeServer &) = delete;
  ~OpenLessPipeServer();

  HRESULT Start(OpenLessTextService *service);
  void Stop();

private:
  void Run() noexcept;
  void RunLoop(bool *startup_reported);
  void ReportStartupResult(HRESULT result) noexcept;
  bool WaitForClient(HANDLE pipe);
  void WaitForClientDisconnect(HANDLE pipe);
  bool ReadJsonLine(HANDLE pipe, std::string *line);
  void HandleSubmitLine(HANDLE pipe, const std::string &line);
  bool WriteResult(HANDLE pipe, const std::wstring &session_id, const wchar_t *status,
                   const wchar_t *error_code);
  // Issues one overlapped read or write and waits until it completes or the
  // stop event is signaled. Returns true only on successful completion, with
  // the transferred byte count stored in *bytes. Overlapped I/O keeps every
  // pipe wait cancelable so Stop() never blocks the host process UI thread.
  bool RunOverlapped(HANDLE pipe, bool is_write, void *buffer, DWORD length, DWORD *bytes);
  void ResetServerState();

  std::atomic<bool> stop_requested_{false};
  std::thread thread_;
  HANDLE startup_event_ = nullptr;
  HANDLE stop_event_ = nullptr;
  HANDLE io_event_ = nullptr;
  HRESULT startup_result_ = E_PENDING;
  std::wstring pipe_name_;
  OpenLessTextService *service_ = nullptr;
};
