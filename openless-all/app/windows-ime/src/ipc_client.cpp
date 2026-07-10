#include "ipc_client.h"

#include <cstdint>
#include <string>

#include "text_service.h"

namespace {

constexpr wchar_t kPipeNamePrefix[] = L"\\\\.\\pipe\\OpenLessImeSubmit";
constexpr DWORD kPipeBufferSize = 4096;
constexpr size_t kMaxJsonLineBytes = 64 * 1024;

struct SubmitMessage {
  std::wstring type;
  std::wstring session_id;
  std::wstring text;
  int protocol_version = 0;
  bool has_type = false;
  bool has_session_id = false;
  bool has_text = false;
  bool has_protocol_version = false;
};

std::wstring HResultErrorCode(HRESULT hr) {
  constexpr wchar_t kHexDigits[] = L"0123456789ABCDEF";
  auto value = static_cast<unsigned long>(hr);
  std::wstring code = L"hresult:0x";
  for (int shift = 28; shift >= 0; shift -= 4) {
    code.push_back(kHexDigits[(value >> shift) & 0xF]);
  }
  return code;
}

std::wstring PipeNameForCurrentThread() {
  std::wstring name = kPipeNamePrefix;
  name += L"-";
  name += std::to_wstring(GetCurrentProcessId());
  name += L"-";
  name += std::to_wstring(GetCurrentThreadId());
  return name;
}

bool AppendUtf8AsWide(const char* data,
                      int length,
                      std::wstring* output) {
  if (length == 0) {
    return true;
  }

  const int required = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, data,
                                           length, nullptr, 0);
  if (required <= 0) {
    return false;
  }

  const size_t old_size = output->size();
  output->resize(old_size + static_cast<size_t>(required));
  return MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, data, length,
                             output->data() + old_size, required) == required;
}

bool WideToUtf8(const std::wstring& value, std::string* output) {
  output->clear();
  if (value.empty()) {
    return true;
  }

  const int required = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS,
                                           value.c_str(),
                                           static_cast<int>(value.size()),
                                           nullptr, 0, nullptr, nullptr);
  if (required <= 0) {
    return false;
  }

  output->resize(static_cast<size_t>(required));
  return WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.c_str(),
                             static_cast<int>(value.size()), output->data(),
                             required, nullptr, nullptr) == required;
}

void SkipWhitespace(const std::string& json, size_t* pos) {
  while (*pos < json.size()) {
    const char c = json[*pos];
    if (c != ' ' && c != '\t' && c != '\r' && c != '\n') {
      return;
    }
    ++(*pos);
  }
}

int HexDigit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  if (c >= 'A' && c <= 'F') {
    return c - 'A' + 10;
  }
  return -1;
}

bool ParseJsonString(const std::string& json,
                     size_t* pos,
                     std::wstring* value) {
  value->clear();
  if (*pos >= json.size() || json[*pos] != '"') {
    return false;
  }
  ++(*pos);

  size_t segment_start = *pos;
  while (*pos < json.size()) {
    const char c = json[*pos];
    if (static_cast<unsigned char>(c) < 0x20) {
      return false;
    }

    if (c == '"') {
      if (!AppendUtf8AsWide(json.data() + segment_start,
                            static_cast<int>(*pos - segment_start), value)) {
        return false;
      }
      ++(*pos);
      return true;
    }

    if (c != '\\') {
      ++(*pos);
      continue;
    }

    if (!AppendUtf8AsWide(json.data() + segment_start,
                          static_cast<int>(*pos - segment_start), value)) {
      return false;
    }

    ++(*pos);
    if (*pos >= json.size()) {
      return false;
    }

    const char escaped = json[*pos];
    switch (escaped) {
      case '"':
      case '\\':
      case '/':
        value->push_back(static_cast<wchar_t>(escaped));
        ++(*pos);
        break;
      case 'b':
        value->push_back(L'\b');
        ++(*pos);
        break;
      case 'f':
        value->push_back(L'\f');
        ++(*pos);
        break;
      case 'n':
        value->push_back(L'\n');
        ++(*pos);
        break;
      case 'r':
        value->push_back(L'\r');
        ++(*pos);
        break;
      case 't':
        value->push_back(L'\t');
        ++(*pos);
        break;
      case 'u': {
        if (*pos + 4 >= json.size()) {
          return false;
        }
        uint32_t code_unit = 0;
        for (int i = 1; i <= 4; ++i) {
          const int digit = HexDigit(json[*pos + static_cast<size_t>(i)]);
          if (digit < 0) {
            return false;
          }
          code_unit = (code_unit << 4) | static_cast<uint32_t>(digit);
        }
        value->push_back(static_cast<wchar_t>(code_unit));
        *pos += 5;
        break;
      }
      default:
        return false;
    }

    segment_start = *pos;
  }

  return false;
}

bool ParseJsonInteger(const std::string& json, size_t* pos, int* value) {
  if (*pos >= json.size() || json[*pos] < '0' || json[*pos] > '9') {
    return false;
  }

  int parsed = 0;
  while (*pos < json.size() && json[*pos] >= '0' && json[*pos] <= '9') {
    parsed = parsed * 10 + (json[*pos] - '0');
    ++(*pos);
  }

  *value = parsed;
  return true;
}

bool SkipJsonValue(const std::string& json, size_t* pos) {
  std::wstring ignored;
  if (*pos >= json.size()) {
    return false;
  }
  if (json[*pos] == '"') {
    return ParseJsonString(json, pos, &ignored);
  }
  while (*pos < json.size() && json[*pos] != ',' && json[*pos] != '}') {
    ++(*pos);
  }
  return true;
}

bool ParseSubmitMessage(const std::string& json, SubmitMessage* message) {
  size_t pos = 0;
  SkipWhitespace(json, &pos);
  if (pos >= json.size() || json[pos] != '{') {
    return false;
  }
  ++pos;

  while (true) {
    SkipWhitespace(json, &pos);
    if (pos < json.size() && json[pos] == '}') {
      ++pos;
      break;
    }

    std::wstring key;
    if (!ParseJsonString(json, &pos, &key)) {
      return false;
    }

    SkipWhitespace(json, &pos);
    if (pos >= json.size() || json[pos] != ':') {
      return false;
    }
    ++pos;
    SkipWhitespace(json, &pos);

    if (key == L"type") {
      message->has_type = ParseJsonString(json, &pos, &message->type);
      if (!message->has_type) {
        return false;
      }
    } else if (key == L"sessionId") {
      message->has_session_id =
          ParseJsonString(json, &pos, &message->session_id);
      if (!message->has_session_id) {
        return false;
      }
    } else if (key == L"text") {
      message->has_text = ParseJsonString(json, &pos, &message->text);
      if (!message->has_text) {
        return false;
      }
    } else if (key == L"protocolVersion") {
      message->has_protocol_version =
          ParseJsonInteger(json, &pos, &message->protocol_version);
      if (!message->has_protocol_version) {
        return false;
      }
    } else if (!SkipJsonValue(json, &pos)) {
      return false;
    }

    SkipWhitespace(json, &pos);
    if (pos < json.size() && json[pos] == ',') {
      ++pos;
      continue;
    }
    if (pos < json.size() && json[pos] == '}') {
      ++pos;
      break;
    }
    return false;
  }

  SkipWhitespace(json, &pos);
  return pos == json.size();
}

std::wstring EscapeJsonString(const std::wstring& value) {
  std::wstring escaped;
  for (wchar_t ch : value) {
    switch (ch) {
      case L'"':
        escaped += L"\\\"";
        break;
      case L'\\':
        escaped += L"\\\\";
        break;
      case L'\b':
        escaped += L"\\b";
        break;
      case L'\f':
        escaped += L"\\f";
        break;
      case L'\n':
        escaped += L"\\n";
        break;
      case L'\r':
        escaped += L"\\r";
        break;
      case L'\t':
        escaped += L"\\t";
        break;
      default:
        if (ch < 0x20) {
          wchar_t buffer[7] = {};
          swprintf_s(buffer, L"\\u%04X", static_cast<unsigned int>(ch));
          escaped += buffer;
        } else {
          escaped.push_back(ch);
        }
        break;
    }
  }
  return escaped;
}

}  // namespace

OpenLessPipeServer::OpenLessPipeServer() = default;

OpenLessPipeServer::~OpenLessPipeServer() {
  Stop();
}

void OpenLessPipeServer::Start(OpenLessTextService* service) {
  if (service == nullptr || thread_.joinable()) {
    return;
  }

  stop_requested_.store(false);

  // Manual-reset events: stop_event_ wakes every overlapped wait the moment
  // Stop() runs; io_event_ is the completion event reused by each overlapped
  // operation on the worker thread.
  stop_event_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  io_event_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  if (stop_event_ == nullptr || io_event_ == nullptr) {
    if (stop_event_ != nullptr) {
      CloseHandle(stop_event_);
      stop_event_ = nullptr;
    }
    if (io_event_ != nullptr) {
      CloseHandle(io_event_);
      io_event_ = nullptr;
    }
    return;
  }

  pipe_name_ = PipeNameForCurrentThread();
  service_ = service;
  service_->AddRef();
  thread_ = std::thread(&OpenLessPipeServer::Run, this);
}

void OpenLessPipeServer::Stop() {
  stop_requested_.store(true);
  if (stop_event_ != nullptr) {
    SetEvent(stop_event_);
  }

  // The worker parks in WaitForMultipleObjects on {io_event_, stop_event_}, so
  // signaling stop_event_ wakes it deterministically. join() then returns at
  // once instead of blocking the host UI thread on an uncancelable
  // ConnectNamedPipe wait — the root cause of the explorer.exe AppHang.
  if (thread_.joinable()) {
    thread_.join();
  }

  if (io_event_ != nullptr) {
    CloseHandle(io_event_);
    io_event_ = nullptr;
  }
  if (stop_event_ != nullptr) {
    CloseHandle(stop_event_);
    stop_event_ = nullptr;
  }

  if (service_ != nullptr) {
    service_->Release();
    service_ = nullptr;
  }
}

void OpenLessPipeServer::Run() {
  const std::wstring pipe_name = pipe_name_;
  while (!stop_requested_.load()) {
    HANDLE pipe = CreateNamedPipeW(
        pipe_name.c_str(), PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
        PIPE_TYPE_MESSAGE | PIPE_READMODE_BYTE | PIPE_WAIT, 1,
        kPipeBufferSize, kPipeBufferSize, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
      return;
    }

    if (WaitForClient(pipe) && !stop_requested_.load()) {
      std::string line;
      if (ReadJsonLine(pipe, &line)) {
        HandleSubmitLine(pipe, line);
      }
    }

    FlushFileBuffers(pipe);
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
  }
}

bool OpenLessPipeServer::WaitForClient(HANDLE pipe) {
  OVERLAPPED overlapped = {};
  overlapped.hEvent = io_event_;
  ResetEvent(io_event_);

  if (ConnectNamedPipe(pipe, &overlapped)) {
    // Overlapped ConnectNamedPipe is not expected to return TRUE, but if it
    // does the client is already connected.
    return true;
  }

  const DWORD error = GetLastError();
  if (error == ERROR_PIPE_CONNECTED) {
    return true;  // A client connected before ConnectNamedPipe was called.
  }
  if (error != ERROR_IO_PENDING) {
    return false;  // Genuine failure establishing the connection.
  }

  const HANDLE wait_handles[2] = {io_event_, stop_event_};
  const DWORD wait = WaitForMultipleObjects(2, wait_handles, FALSE, INFINITE);
  if (wait == WAIT_OBJECT_0) {
    DWORD transferred = 0;
    return GetOverlappedResult(pipe, &overlapped, &transferred, FALSE) != 0;
  }

  // Stop requested (or the wait failed): cancel the pending connect and block
  // until the kernel is finished with the stack OVERLAPPED before returning.
  CancelIoEx(pipe, &overlapped);
  DWORD transferred = 0;
  GetOverlappedResult(pipe, &overlapped, &transferred, TRUE);
  return false;
}

bool OpenLessPipeServer::RunOverlapped(HANDLE pipe,
                                       bool is_write,
                                       void* buffer,
                                       DWORD length,
                                       DWORD* bytes) {
  *bytes = 0;

  OVERLAPPED overlapped = {};
  overlapped.hEvent = io_event_;
  ResetEvent(io_event_);

  const BOOL started =
      is_write ? WriteFile(pipe, buffer, length, nullptr, &overlapped)
               : ReadFile(pipe, buffer, length, nullptr, &overlapped);
  if (!started) {
    const DWORD error = GetLastError();
    if (error != ERROR_IO_PENDING) {
      return false;  // e.g. ERROR_BROKEN_PIPE once the client disconnects.
    }

    const HANDLE wait_handles[2] = {io_event_, stop_event_};
    const DWORD wait = WaitForMultipleObjects(2, wait_handles, FALSE, INFINITE);
    if (wait != WAIT_OBJECT_0) {
      // Stop requested (or the wait failed): cancel and drain before the stack
      // OVERLAPPED and caller buffer go out of scope.
      CancelIoEx(pipe, &overlapped);
      DWORD drained = 0;
      GetOverlappedResult(pipe, &overlapped, &drained, TRUE);
      return false;
    }
  }

  return GetOverlappedResult(pipe, &overlapped, bytes, FALSE) != 0;
}

bool OpenLessPipeServer::ReadJsonLine(HANDLE pipe, std::string* line) {
  line->clear();
  char buffer[1024] = {};

  while (!stop_requested_.load() && line->size() < kMaxJsonLineBytes) {
    DWORD bytes_read = 0;
    if (!RunOverlapped(pipe, /*is_write=*/false, buffer, sizeof(buffer),
                       &bytes_read)) {
      return !line->empty();
    }

    if (bytes_read == 0) {
      return !line->empty();
    }

    for (DWORD i = 0; i < bytes_read; ++i) {
      if (buffer[i] == '\n') {
        return true;
      }
      line->push_back(buffer[i]);
      if (line->size() >= kMaxJsonLineBytes) {
        return true;
      }
    }
  }

  return !line->empty();
}

void OpenLessPipeServer::HandleSubmitLine(HANDLE pipe, const std::string& line) {
  SubmitMessage message;
  if (!ParseSubmitMessage(line, &message) || !message.has_type ||
      !message.has_protocol_version || !message.has_session_id ||
      !message.has_text || message.protocol_version != 1 ||
      message.type != L"submitText") {
    WriteResult(pipe, message.session_id, L"failed", L"protocolError");
    return;
  }

  if (service_ == nullptr) {
    WriteResult(pipe, message.session_id, L"failed", L"serviceUnavailable");
    return;
  }

  const HRESULT hr =
      service_->SubmitTextFromPipe(message.session_id, message.text);
  if (SUCCEEDED(hr)) {
    WriteResult(pipe, message.session_id, L"committed", nullptr);
  } else {
    const std::wstring error_code = HResultErrorCode(hr);
    WriteResult(pipe, message.session_id, L"rejected", error_code.c_str());
  }
}

bool OpenLessPipeServer::WriteResult(HANDLE pipe,
                                     const std::wstring& session_id,
                                     const wchar_t* status,
                                     const wchar_t* error_code) {
  std::wstring response = L"{\"type\":\"submitResult\",\"protocolVersion\":1,";
  response += L"\"sessionId\":\"";
  response += EscapeJsonString(session_id);
  response += L"\",\"status\":\"";
  response += status;
  response += L"\",\"errorCode\":";
  if (error_code == nullptr) {
    response += L"null";
  } else {
    response += L"\"";
    response += error_code;
    response += L"\"";
  }
  response += L"}\n";

  std::string utf8_response;
  if (!WideToUtf8(response, &utf8_response)) {
    return false;
  }

  DWORD bytes_written = 0;
  return RunOverlapped(pipe, /*is_write=*/true, utf8_response.data(),
                       static_cast<DWORD>(utf8_response.size()),
                       &bytes_written) &&
         bytes_written == utf8_response.size();
}
