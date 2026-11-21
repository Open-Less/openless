#import <Foundation/Foundation.h>
#import "../RimeBridge/OLRimeSession.h"
#include <cassert>
#include <string>

static OLRimeSnapshot *type(OLRimeSession *session, const char *keys) {
    OLRimeSnapshot *state = nil;
    for (char key : std::string(keys)) state = [session processKey:key];
    return state;
}
int main(int argc, const char *argv[]) {
    @autoreleasepool {
        assert(argc == 2);
        NSString *root = [NSTemporaryDirectory() stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
        NSString *user = [root stringByAppendingPathComponent:@"user"];
        NSString *shared = [root stringByAppendingPathComponent:@"shared"];
        NSString *source = [NSString stringWithUTF8String:argv[1]];
        NSFileManager *fm = NSFileManager.defaultManager;
        [fm createDirectoryAtPath:shared withIntermediateDirectories:YES attributes:nil error:nil];
        for (NSString *name in @[@"default.yaml", @"pinyin_simp.schema.yaml", @"pinyin_simp.dict.yaml"]) {
            assert([fm copyItemAtPath:[source stringByAppendingPathComponent:name] toPath:[shared stringByAppendingPathComponent:name] error:nil]);
        }
        assert([fm createSymbolicLinkAtPath:[shared stringByAppendingPathComponent:@"build"] withDestinationPath:[source stringByAppendingPathComponent:@"build"] error:nil]);
        NSError *error = nil;
        OLRimeSession *session = [[OLRimeSession alloc] initWithSharedDataPath:shared userDataPath:user error:&error];
        if (!session) { NSLog(@"%@", error); return 1; }
        NSLog(@"pinyin 女绿: %@", [OLRimeSession pinyinForPhrase:@"女绿"]);
        assert([[OLRimeSession pinyinForPhrase:@"女绿"] isEqualToString:@"nv lv"]);
        assert([OLRimeSession pinyinForPhrase:@"OpenLess"] == nil);
        assert([OLRimeSession pinyinForPhrase:@"词条\n注入"] == nil);
        assert([OLRimeSession pinyinForPhrase:@"测试123"] == nil);
        NSString *phrase = @"玄穹量子桥";
        NSLog(@"user: %@ code: %@", user, [OLRimeSession pinyinForPhrase:phrase]);
        NSString *sentinel = [user stringByAppendingPathComponent:@"personal-learning-sentinel"];
        [@"untouched" writeToFile:sentinel atomically:YES encoding:NSUTF8StringEncoding error:nil];
        OLRimeVocabularyUpdate update = [session importConfirmedPhrases:@[phrase, @"OpenLess"] revision:@"localOffline:1" error:&error];
        if (update != OLRimeVocabularyUpdateApplied) { NSLog(@"%@", error); return 2; }
        OLRimeSnapshot *state = type(session, "nihao");
        assert([state.candidates containsObject:@"你好"]);
        [session clearComposition];
        state = type(session, "xuanqiongliangziqiao");
        NSLog(@"confirmed candidates: %@", state.candidates);
        assert([state.candidates containsObject:phrase]);
        // Revocation must NOT destroy the currently active composition.
        assert([session importConfirmedPhrases:@[] revision:@"localOffline:2" error:&error] == OLRimeVocabularyUpdateDeferred);
        state = [session selectCandidate:[state.candidates indexOfObject:phrase]];
        assert([state.committedText isEqualToString:phrase]);
        assert(state.preedit.length == 0);
        state = type(session, "xuanqiongliangziqiao");
        assert(![state.candidates containsObject:phrase]);
        [session clearComposition];
        assert([session importConfirmedPhrases:@[] revision:@"localOffline:2" error:&error] == OLRimeVocabularyUpdateUnchanged);
        state = type(session, "nihao");
        assert([state.candidates containsObject:@"你好"]);
        [session clearComposition];
        // Latest snapshot supersedes a pending update, even if it matches current revision.
        type(session, "ni");
        assert([session importConfirmedPhrases:@[phrase] revision:@"localOffline:3" error:&error] == OLRimeVocabularyUpdateDeferred);
        assert([session importConfirmedPhrases:@[] revision:@"localOffline:2" error:&error] == OLRimeVocabularyUpdateUnchanged);
        [session clearComposition];
        state = type(session, "xuanqiongliangziqiao");
        assert(![state.candidates containsObject:phrase]);
        [session clearComposition];
        assert([[NSString stringWithContentsOfFile:sentinel encoding:NSUTF8StringEncoding error:nil] isEqualToString:@"untouched"]);
        for (NSString *name in [NSFileManager.defaultManager contentsOfDirectoryAtPath:[user stringByAppendingPathComponent:@"build"] error:nil]) {
            assert(![name hasPrefix:@"ol_vocabulary_"]);
        }
        // Generation reuse after metadata recovery cannot retain stale contents.
        assert([session importConfirmedPhrases:@[phrase] revision:@"localOffline:2" error:&error] == OLRimeVocabularyUpdateApplied);
        state = type(session, "xuanqiongliangziqiao");
        assert([state.candidates containsObject:phrase]);
        [session clearComposition];
        assert([session importConfirmedPhrases:@[] revision:@"localOffline:2" error:&error] == OLRimeVocabularyUpdateApplied);
        // Simulated schema source failure touches only the temporary copy.
        [fm removeItemAtPath:[shared stringByAppendingPathComponent:@"pinyin_simp.schema.yaml"] error:nil];
        error = nil;
        assert([session importConfirmedPhrases:@[phrase] revision:@"localOffline:4" error:&error] == OLRimeVocabularyUpdateFailed);
        assert(error != nil && session.vocabularyImportError != nil);
        state = type(session, "nihao");
        assert([state.candidates containsObject:@"你好"]);
        [session clearComposition];
        // A temporary deployment failure must not cache the same revision as done.
        assert([fm copyItemAtPath:[source stringByAppendingPathComponent:@"pinyin_simp.schema.yaml"]
                          toPath:[shared stringByAppendingPathComponent:@"pinyin_simp.schema.yaml"] error:nil]);
        error = nil;
        assert([session importConfirmedPhrases:@[phrase] revision:@"localOffline:4" error:&error] == OLRimeVocabularyUpdateApplied);
        assert(error == nil && session.vocabularyImportError == nil);
        state = type(session, "xuanqiongliangziqiao");
        assert([state.candidates containsObject:phrase]);
        [session clearComposition];
        // A second reader appearing after admission must not deploy or revive
        // stale pending words; the newest valid revocation wins.
        type(session, "ni");
        assert([session importConfirmedPhrases:@[] revision:@"localOffline:5" error:&error] == OLRimeVocabularyUpdateDeferred);
        OLRimeSession *second = [[OLRimeSession alloc] initWithSharedDataPath:shared userDataPath:user error:&error];
        assert(second != nil);
        assert([session importConfirmedPhrases:@[phrase] revision:@"localOffline:6" error:&error] == OLRimeVocabularyUpdateDeferred);
        [session clearComposition];
        assert(session.vocabularyImportError != nil);
        assert([session importConfirmedPhrases:@[] revision:@"localOffline:7" error:&error] == OLRimeVocabularyUpdateDeferred);
        assert(error == nil && session.vocabularyImportError == nil);
        second = nil;
        [session clearComposition];
        assert(session.vocabularyImportError == nil);
        state = type(session, "xuanqiongliangziqiao");
        assert(![state.candidates containsObject:phrase]);
        [session clearComposition];
        for (NSString *name in [fm contentsOfDirectoryAtPath:user error:nil]) {
            assert(!([name hasPrefix:@"ol_vocabulary_"] && [name hasSuffix:@".userdb"]));
        }
        assert([fm fileExistsAtPath:[user stringByAppendingPathComponent:@"pinyin_simp.userdb"]]);
        session = nil;
        [fm removeItemAtPath:root error:nil];
        puts("Rime vocabulary: real phrase candidate, commit, deferred revoke, latest-wins, isolated cleanup and basic pinyin passed.");
    }
}
