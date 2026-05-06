package com.openless.android;

interface PolishProvider {
    String polish(String raw, PolishMode mode, java.util.List<String> hotwords) throws Exception;

    String translate(String raw, String targetLanguage, java.util.List<String> hotwords,
                     java.util.List<String> workingLanguages) throws Exception;
}
