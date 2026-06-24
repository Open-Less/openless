#pragma once

#include <atomic>
#include <mutex>
#include <string>
#include <thread>
#include <windows.h>

class OpenLessTextService;

class OpenLessPipeServer {
 public:
  OpenLessPipeServer();
  OpenLessPipeServer(const OpenLessPipeServer&) = delete;
  OpenLessPipeServer& operator=(const OpenLessPipeServer&) = delete;
  ~OpenLessPipeServer();

  void Start(OpenLessTextService* service);
  void Stop();

 private:
  void Run();
  bool ReadJsonLine(HANDLE pipe, std::string* line);
  void HandleSubmitLine(HANDLE pipe, const std::string& line);
  bool WriteResult(HANDLE pipe,
                   const std::wstring& session_id,
                   const wchar_t* status,
                   const wchar_t* error_code);
  void WakePipe();

  std::atomic<bool> stop_requested_{false};
  std::thread thread_;
  std::mutex pipe_mutex_;
  HANDLE pipe_handle_ = INVALID_HANDLE_VALUE;
  HANDLE shutdown_event_ = nullptr;
  std::wstring pipe_name_;
  OpenLessTextService* service_ = nullptr;
};
