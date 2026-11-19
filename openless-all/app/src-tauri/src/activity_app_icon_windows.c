#define COBJMACROS
#define WIN32_LEAN_AND_MEAN
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#include <windows.h>
#include <objbase.h>
#include <shellapi.h>
#include <wincodec.h>
#include <stdint.h>

typedef void (*OpenLessIconReceiver)(void *, const uint8_t *, size_t);

uint32_t openless_windows_activity_app_path(uint32_t pid, wchar_t *path, uint32_t capacity) {
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!process) return 0;
    DWORD length = capacity;
    BOOL ok = QueryFullProcessImageNameW(process, 0, path, &length);
    CloseHandle(process);
    return ok ? length : 0;
}

void openless_windows_activity_app_icon(const wchar_t *path, void *output, OpenLessIconReceiver receive) {
    // This is called on a blocking worker with an executable obtained from a local process.
    // Never inspect icon resources on a UNC path or a mapped remote drive.
    if (!path[0] || path[1] != L':' || path[2] != L'\\') return;
    wchar_t drive[] = {path[0], L':', L'\\', 0};
    UINT driveType = GetDriveTypeW(drive);
    if (driveType == DRIVE_REMOTE || driveType == DRIVE_NO_ROOT_DIR || driveType == DRIVE_UNKNOWN) return;
    HRESULT apartment = CoInitializeEx(NULL, COINIT_MULTITHREADED);
    if (FAILED(apartment) && apartment != RPC_E_CHANGED_MODE) return;
    HICON icon = NULL;
    IWICImagingFactory *factory = NULL;
    IWICBitmap *bitmap = NULL;
    IWICBitmapEncoder *encoder = NULL;
    IWICBitmapFrameEncode *frame = NULL;
    IStream *stream = NULL;

    if (ExtractIconExW(path, 0, &icon, NULL, 1) != 1 || !icon) goto cleanup;
    if (FAILED(CoCreateInstance(&CLSID_WICImagingFactory, NULL, CLSCTX_INPROC_SERVER,
                                &IID_IWICImagingFactory, (void **)&factory))) goto cleanup;
    if (FAILED(IWICImagingFactory_CreateBitmapFromHICON(factory, icon, &bitmap))) goto cleanup;
    UINT width, height;
    if (FAILED(IWICBitmap_GetSize(bitmap, &width, &height)) ||
        !width || !height || width > 256 || height > 256) goto cleanup;
    if (FAILED(CreateStreamOnHGlobal(NULL, TRUE, &stream))) goto cleanup;
    if (FAILED(IWICImagingFactory_CreateEncoder(factory, &GUID_ContainerFormatPng, NULL, &encoder))) goto cleanup;
    if (FAILED(IWICBitmapEncoder_Initialize(encoder, stream, WICBitmapEncoderNoCache))) goto cleanup;
    if (FAILED(IWICBitmapEncoder_CreateNewFrame(encoder, &frame, NULL))) goto cleanup;
    if (FAILED(IWICBitmapFrameEncode_Initialize(frame, NULL))) goto cleanup;
    if (FAILED(IWICBitmapFrameEncode_SetSize(frame, width, height))) goto cleanup;
    WICPixelFormatGUID format = GUID_WICPixelFormat32bppBGRA;
    if (FAILED(IWICBitmapFrameEncode_SetPixelFormat(frame, &format))) goto cleanup;
    if (FAILED(IWICBitmapFrameEncode_WriteSource(frame, (IWICBitmapSource *)bitmap, NULL))) goto cleanup;
    if (FAILED(IWICBitmapFrameEncode_Commit(frame))) goto cleanup;
    if (FAILED(IWICBitmapEncoder_Commit(encoder))) goto cleanup;

    STATSTG stat;
    HGLOBAL memory;
    if (FAILED(IStream_Stat(stream, &stat, STATFLAG_NONAME)) ||
        stat.cbSize.QuadPart > 131072 || !stat.cbSize.QuadPart ||
        FAILED(GetHGlobalFromStream(stream, &memory))) goto cleanup;
    const uint8_t *data = GlobalLock(memory);
    if (data) {
        receive(output, data, (size_t)stat.cbSize.QuadPart);
        GlobalUnlock(memory);
    }

cleanup:
    if (frame) IWICBitmapFrameEncode_Release(frame);
    if (encoder) IWICBitmapEncoder_Release(encoder);
    if (stream) IStream_Release(stream);
    if (bitmap) IWICBitmap_Release(bitmap);
    if (factory) IWICImagingFactory_Release(factory);
    if (icon) DestroyIcon(icon);
    if (SUCCEEDED(apartment)) CoUninitialize();
}
