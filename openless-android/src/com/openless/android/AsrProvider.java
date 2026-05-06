package com.openless.android;

interface AsrProvider {
    RawTranscript transcribe(AudioRecorder.Recording recording) throws Exception;
}
