#pragma once

#include <msctf.h>
#include <windows.h>

#include "guids.h"

// Solid red underline for the live ASR composition, matching the issue mock.
void FillOpenLessInputDisplayAttribute(TF_DISPLAYATTRIBUTE *attribute);

// Sets GUID_PROP_ATTRIBUTE on `range` to the OpenLess input display attribute.
// Failures only affect appearance; the composition text is still written.
HRESULT ApplyOpenLessInputDisplayAttribute(TfEditCookie edit_cookie, ITfContext *context,
                                           ITfRange *range);

// EndComposition only clears GUID_PROP_COMPOSING, not the display attribute.
HRESULT ClearOpenLessInputDisplayAttribute(TfEditCookie edit_cookie, ITfContext *context,
                                           ITfRange *range);

class OpenLessDisplayAttributeInfo final : public ITfDisplayAttributeInfo {
public:
  OpenLessDisplayAttributeInfo();
  OpenLessDisplayAttributeInfo(const OpenLessDisplayAttributeInfo &) = delete;
  OpenLessDisplayAttributeInfo &operator=(const OpenLessDisplayAttributeInfo &) = delete;
  ~OpenLessDisplayAttributeInfo();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;

  STDMETHODIMP GetGUID(GUID *guid) override;
  STDMETHODIMP GetDescription(BSTR *description) override;
  STDMETHODIMP GetAttributeInfo(TF_DISPLAYATTRIBUTE *attribute) override;
  STDMETHODIMP SetAttributeInfo(const TF_DISPLAYATTRIBUTE *attribute) override;
  STDMETHODIMP Reset() override;

private:
  LONG ref_count_ = 1;
};

class OpenLessEnumDisplayAttributeInfo final : public IEnumTfDisplayAttributeInfo {
public:
  OpenLessEnumDisplayAttributeInfo();
  OpenLessEnumDisplayAttributeInfo(const OpenLessEnumDisplayAttributeInfo &) = delete;
  OpenLessEnumDisplayAttributeInfo &operator=(const OpenLessEnumDisplayAttributeInfo &) = delete;
  ~OpenLessEnumDisplayAttributeInfo();

  STDMETHODIMP QueryInterface(REFIID iid, void **object) override;
  STDMETHODIMP_(ULONG) AddRef() override;
  STDMETHODIMP_(ULONG) Release() override;
  STDMETHODIMP Clone(IEnumTfDisplayAttributeInfo **enum_info) override;
  STDMETHODIMP Next(ULONG count, ITfDisplayAttributeInfo **info, ULONG *fetched) override;
  STDMETHODIMP Reset() override;
  STDMETHODIMP Skip(ULONG count) override;

private:
  LONG ref_count_ = 1;
  ULONG index_ = 0;
};
