// Keep new tap APIs behind Clang availability checks; ordinary recording still runs on 14.0.
#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#include <mach/mach_time.h>
#include <stdlib.h>

typedef void (*PlaybackCallback)(void *, const AudioBufferList *, uint64_t);
typedef struct {
    AudioObjectID tap;
    AudioObjectID aggregate;
    AudioDeviceIOProcID proc;
    PlaybackCallback callback;
    void *context;
    mach_timebase_info_data_t timebase;
    bool running;
} PlaybackReference;

static OSStatus playback_io(AudioObjectID device, const AudioTimeStamp *now,
        const AudioBufferList *input, const AudioTimeStamp *input_time,
        AudioBufferList *output, const AudioTimeStamp *output_time, void *context) {
    PlaybackReference *reference=context;
    uint64_t host=mach_absolute_time();
    uint64_t captured=(input_time && (input_time->mFlags & kAudioTimeStampHostTimeValid)) ? input_time->mHostTime : host;
    uint64_t age=host>captured ? (uint64_t)((long double)(host-captured)*reference->timebase.numer/reference->timebase.denom) : 0;
    reference->callback(reference->context,input,age);
    return noErr;
}

void openless_playback_destroy(void *handle) {
    if (!handle) return;
    PlaybackReference *reference=handle;
    if (reference->running) AudioDeviceStop(reference->aggregate,reference->proc);
    if (reference->proc) AudioDeviceDestroyIOProcID(reference->aggregate,reference->proc);
    if (@available(macOS 14.6, *)) {
        if (reference->aggregate) AudioHardwareDestroyAggregateDevice(reference->aggregate);
        if (reference->tap) AudioHardwareDestroyProcessTap(reference->tap);
    }
    free(reference);
}

void *openless_playback_create(uint32_t *rate, int32_t *status) {
    *status=-1;
    if (@available(macOS 14.6, *)) {
        @autoreleasepool {
            PlaybackReference *reference=calloc(1,sizeof(*reference));
            if (!reference) return NULL;
            mach_timebase_info(&reference->timebase);
            AudioObjectID output=0;UInt32 size=sizeof(output);
            AudioObjectPropertyAddress property={kAudioHardwarePropertyDefaultOutputDevice,kAudioObjectPropertyScopeGlobal,kAudioObjectPropertyElementMain};
            *status=AudioObjectGetPropertyData(kAudioObjectSystemObject,&property,0,NULL,&size,&output);
            if (*status!=noErr || output==kAudioObjectUnknown) {openless_playback_destroy(reference);return NULL;}
            CFStringRef uid=NULL;size=sizeof(uid);
            property.mSelector=kAudioDevicePropertyDeviceUID;
            *status=AudioObjectGetPropertyData(output,&property,0,NULL,&size,&uid);
            if (*status!=noErr || !uid) {openless_playback_destroy(reference);return NULL;}
            NSString *device_uid=CFBridgingRelease(uid);
            // Unlike CPAL's generic input path, this always taps output, even if
            // the selected audio interface also has microphone input channels.
            CATapDescription *tap=[[CATapDescription alloc] initWithProcesses:@[] andDeviceUID:device_uid withStream:0];
            [tap setPrivate:YES];[tap setExclusive:YES];[tap setMuteBehavior:CATapUnmuted];
            *status=AudioHardwareCreateProcessTap(tap,&reference->tap);
            if (*status!=noErr) {openless_playback_destroy(reference);return NULL;}
            AudioStreamBasicDescription format={0};size=sizeof(format);
            property.mSelector=kAudioTapPropertyFormat;
            *status=AudioObjectGetPropertyData(reference->tap,&property,0,NULL,&size,&format);
            if (*status!=noErr || format.mFormatID!=kAudioFormatLinearPCM || !(format.mFormatFlags & kAudioFormatFlagIsFloat) || format.mBitsPerChannel!=32 || format.mSampleRate<=0 || format.mChannelsPerFrame==0) {
                if (*status==noErr) *status=kAudioHardwareUnsupportedOperationError;
                openless_playback_destroy(reference);return NULL;
            }
            NSDictionary *properties=@{
                @kAudioAggregateDeviceNameKey: @"OpenLess playback reference",
                @kAudioAggregateDeviceUIDKey: [[NSUUID UUID] UUIDString],
                @kAudioAggregateDeviceIsPrivateKey: @YES,
                @kAudioAggregateDeviceTapAutoStartKey: @YES,
                @kAudioAggregateDeviceTapListKey: @[@{
                    @kAudioSubTapUIDKey: [[tap UUID] UUIDString],
                    @kAudioSubTapDriftCompensationKey: @YES
                }]
            };
            *status=AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)properties,&reference->aggregate);
            if (*status!=noErr) {openless_playback_destroy(reference);return NULL;}
            *rate=(uint32_t)format.mSampleRate;
            return reference;
        }
    }
    return NULL;
}

int32_t openless_playback_start(void *handle, PlaybackCallback callback, void *context) {
    PlaybackReference *reference=handle;
    reference->callback=callback;reference->context=context;
    OSStatus status=AudioDeviceCreateIOProcID(reference->aggregate,playback_io,reference,&reference->proc);
    if (status!=noErr) return status;
    status=AudioDeviceStart(reference->aggregate,reference->proc);
    reference->running=status==noErr;
    return status;
}
