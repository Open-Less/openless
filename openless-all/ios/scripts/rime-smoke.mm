#import <Foundation/Foundation.h>
#import "../RimeBridge/OLRimeSession.h"
#include <cassert>
#include <string>

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        assert(argc == 2);
        NSString *user = [NSTemporaryDirectory() stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
        NSError *error = nil;
        OLRimeSession *session = [[OLRimeSession alloc] initWithSharedDataPath:[NSString stringWithUTF8String:argv[1]] userDataPath:user error:&error];
        if (!session) { NSLog(@"%@", error); return 1; }
        for (const auto &pair : {
            std::make_pair("nihao", @"你好"),
            std::make_pair("zhongguo", @"中国"),
            std::make_pair("mingtianjian", @"明天见"),
            std::make_pair("dengyixia", @"等一下"),
            std::make_pair("weixin", @"微信"),
            std::make_pair("pengyouquan", @"朋友圈"),
            std::make_pair("yuyinshuru", @"语音输入"),
        }) {
            OLRimeSnapshot *state = nil;
            for (char key : std::string(pair.first)) state = [session processKey:key];
            assert(state.preedit.length > 0);
            assert([state.candidates containsObject:pair.second]);
            NSUInteger candidate = [state.candidates indexOfObject:pair.second];
            state = [session selectCandidate:candidate];
            assert([state.committedText isEqualToString:pair.second]);
            assert(state.preedit.length == 0 && state.rawInput.length == 0);
        }
        // Original input must survive even when Rime proposes valid Chinese,
        // inserts syllable spaces, or formats v as an umlauted u in its preedit.
        for (const char *input : {"openless", "nihao", "nvse", "xi'an"}) {
            OLRimeSnapshot *state = nil;
            for (char key : std::string(input)) state = [session processKey:key];
            NSString *original = [NSString stringWithUTF8String:input];
            assert([state.rawInput isEqualToString:original]);
            state = [session commitRawInput];
            assert([state.committedText isEqualToString:original]);
            assert(state.preedit.length == 0 && state.rawInput.length == 0);
        }
        [session processKey:'n']; [session processKey:'i'];
        OLRimeSnapshot *space = [session processKey:' '];
        assert(space.committedText.length > 0 && ![space.committedText isEqualToString:@"ni"]);
        assert(space.preedit.length == 0 && space.rawInput.length == 0);
        [session processKey:'n']; [session processKey:'i'];
        OLRimeSnapshot *deleted = [session processKey:0xff08];
        assert(deleted.preedit.length > 0);
        assert([deleted.rawInput isEqualToString:@"n"]);
        assert([session commitRawInput].committedText.length == 1);
        assert([session commitRawInput].committedText.length == 0);
        [session processKey:'n'];
        OLRimeSnapshot *cleared = [session clearComposition];
        assert(cleared.preedit.length == 0 && cleared.rawInput.length == 0 && cleared.committedText.length == 0);
        session = nil;
        [[NSFileManager defaultManager] removeItemAtPath:user error:nil];
        puts("Rime native smoke: real pinyin, candidate/space commit, exact raw input, delete and cancel passed.");
    }
}
