#import "OLRimeSession.h"
#include <rime_api.h>
#include <exception>

@interface OLRimeSnapshot ()
@property(nonatomic, copy, readwrite) NSString *preedit;
@property(nonatomic, copy, readwrite) NSString *rawInput;
@property(nonatomic, copy, readwrite) NSString *committedText;
@property(nonatomic, copy, readwrite) NSArray<NSString *> *candidates;
@property(nonatomic, readwrite) BOOL handled;
@property(nonatomic, readwrite) BOOL hasNextPage;
@end
@implementation OLRimeSnapshot
@end

static NSString *engineSharedPath;
static NSString *engineUserPath;
static NSUInteger liveSessions;

@implementation OLRimeSession {
    RimeSessionId _session;
    NSString *_sharedPath;
    NSString *_userPath;
    NSString *_vocabularyRevision;
    NSArray<NSString *> *_appliedPhrases;
    NSArray<NSString *> *_pendingPhrases;
    NSString *_pendingRevision;
    NSError *_vocabularyImportError;
}
- (NSError *)vocabularyImportError { return _vocabularyImportError; }
+ (NSError *)vocabularyError:(NSString *)message {
    return [NSError errorWithDomain:@"OpenLess.Rime" code:4 userInfo:@{NSLocalizedDescriptionKey:message}];
}
- (instancetype)initWithSharedDataPath:(NSString *)shared userDataPath:(NSString *)user error:(NSError **)error {
    NSParameterAssert(NSThread.isMainThread);
    if (!(self = [super init])) return nil;
    if (engineUserPath && (![engineUserPath isEqualToString:user] || ![engineSharedPath isEqualToString:shared])) {
        if (error) *error = [OLRimeSession vocabularyError:@"Rime 路径已由当前扩展进程固定"];
        return nil;
    }
    _sharedPath = [shared copy]; _userPath = [user copy];
    NSString *table = [shared stringByAppendingPathComponent:@"build/pinyin_simp.table.bin"];
    if (![[NSFileManager defaultManager] fileExistsAtPath:table]) {
        if (error) *error = [NSError errorWithDomain:@"OpenLess.Rime" code:1
                                           userInfo:@{NSLocalizedDescriptionKey: @"拼音资源尚未构建。请运行 scripts/build-rime.sh。"}];
        return nil;
    }
    if (![[NSFileManager defaultManager] createDirectoryAtPath:user withIntermediateDirectories:YES attributes:nil error:error]) return nil;
    try {
        static dispatch_once_t once;
        dispatch_once(&once, ^{
            RIME_STRUCT(RimeTraits, traits);
            traits.shared_data_dir = shared.UTF8String;
            traits.user_data_dir = user.UTF8String;
            traits.prebuilt_data_dir = [[shared stringByAppendingPathComponent:@"build"] UTF8String];
            traits.distribution_name = "OpenLess";
            traits.distribution_code_name = "openless";
            traits.distribution_version = "0.1.0";
            traits.app_name = "rime.openless";
            traits.min_log_level = 3;
            rime_get_api()->setup(&traits);
            rime_get_api()->deployer_initialize(&traits);
            rime_get_api()->initialize(&traits);
            engineSharedPath = [shared copy]; engineUserPath = [user copy];
        });
        _session = rime_get_api()->create_session();
        if (!_session || !rime_get_api()->select_schema(_session, "pinyin_simp")) {
            if (_session) rime_get_api()->destroy_session(_session);
            _session = 0;
            if (error) *error = [NSError errorWithDomain:@"OpenLess.Rime" code:2 userInfo:@{NSLocalizedDescriptionKey:@"无法初始化全拼输入方案"}];
            return nil;
        }
        rime_get_api()->set_option(_session, "ascii_mode", False);
        ++liveSessions;
    } catch (const std::exception &) {
        if (error) *error = [NSError errorWithDomain:@"OpenLess.Rime" code:3 userInfo:@{NSLocalizedDescriptionKey:@"拼音引擎初始化失败"}];
        return nil;
    }
    return self;
}
- (void)dealloc {
    if (_session) { rime_get_api()->destroy_session(_session); --liveSessions; }
}
- (OLRimeSnapshot *)snapshot:(BOOL)handled {
    NSParameterAssert(NSThread.isMainThread);
    OLRimeSnapshot *result = [OLRimeSnapshot new];
    result.preedit = @""; result.rawInput = @""; result.committedText = @""; result.candidates = @[]; result.handled = handled;
    RIME_STRUCT(RimeCommit, commit);
    if (rime_get_api()->get_commit(_session, &commit)) {
        if (commit.text) result.committedText = [NSString stringWithUTF8String:commit.text] ?: @"";
        rime_get_api()->free_commit(&commit);
    }
    const char *input = rime_get_api()->get_input(_session);
    if (input) result.rawInput = [NSString stringWithUTF8String:input] ?: @"";
    RIME_STRUCT(RimeContext, context);
    if (rime_get_api()->get_context(_session, &context)) {
        if (context.composition.preedit) result.preedit = [NSString stringWithUTF8String:context.composition.preedit] ?: @"";
        NSMutableArray<NSString *> *candidates = [NSMutableArray new];
        for (int i = 0; i < context.menu.num_candidates; ++i) {
            const char *text = context.menu.candidates[i].text;
            if (text) [candidates addObject:[NSString stringWithUTF8String:text] ?: @""];
        }
        result.candidates = candidates;
        result.hasNextPage = !context.menu.is_last_page;
        rime_get_api()->free_context(&context);
    }
    if (result.preedit.length == 0 && _pendingPhrases) {
        // Commit is already copied above. Redeploy cannot swallow committed text.
        [self applyPendingVocabulary:nil];
    }
    return result;
}
- (OLRimeSnapshot *)processKey:(int)keyCode {
    return [self snapshot:rime_get_api()->process_key(_session, keyCode, 0)];
}
- (OLRimeSnapshot *)selectCandidate:(NSUInteger)index {
    return [self snapshot:rime_get_api()->select_candidate_on_current_page(_session, index)];
}
- (OLRimeSnapshot *)commitComposition {
    return [self snapshot:rime_get_api()->commit_composition(_session)];
}
- (OLRimeSnapshot *)commitRawInput {
    NSParameterAssert(NSThread.isMainThread);
    const char *input = rime_get_api()->get_input(_session);
    // Copy before clear: Rime owns this buffer. Do not commit the formatted
    // preedit, which may insert spaces or convert a typed v to an umlauted u.
    NSString *rawInput = input ? [NSString stringWithUTF8String:input] : @"";
    rime_get_api()->clear_composition(_session);
    OLRimeSnapshot *result = [self snapshot:YES];
    result.committedText = rawInput ?: @"";
    return result;
}
- (OLRimeSnapshot *)clearComposition {
    rime_get_api()->clear_composition(_session);
    return [self snapshot:YES];
}
- (OLRimeSnapshot *)nextPage {
    return [self snapshot:rime_get_api()->change_page(_session, False)];
}

+ (NSString *)pinyinForPhrase:(NSString *)phrase {
    // This schema is Mandarin full pinyin, not a general language parser.
    // Mixed Latin, digits, punctuation and unsupported Unicode are omitted.
    if (phrase.length == 0 || phrase.length > 64) return nil;
    for (NSUInteger i = 0; i < phrase.length; ++i) {
        unichar c = [phrase characterAtIndex:i];
        if (!((c >= 0x3400 && c <= 0x4dbf) || (c >= 0x4e00 && c <= 0x9fff))) return nil;
    }
    NSMutableString *pinyin = [phrase mutableCopy];
    if (!CFStringTransform((__bridge CFMutableStringRef)pinyin, NULL, kCFStringTransformMandarinLatin, false)) return nil;
    // Preserve the umlauted u before stripping tones; the dictionary uses v.
    // Decompose tone-marked u first, then remove only the tone marks.
    NSString *decomposed = [pinyin decomposedStringWithCanonicalMapping];
    decomposed = [decomposed stringByReplacingOccurrencesOfString:@"u\u0308" withString:@"v"
        options:NSLiteralSearch range:NSMakeRange(0, decomposed.length)];
    pinyin = [decomposed mutableCopy];
    if (!CFStringTransform((__bridge CFMutableStringRef)pinyin, NULL, kCFStringTransformStripCombiningMarks, false)) return nil;
    NSArray<NSString *> *syllables = [[pinyin lowercaseString] componentsSeparatedByCharactersInSet:NSCharacterSet.whitespaceCharacterSet];
    NSMutableArray<NSString *> *codes = [NSMutableArray new];
    NSCharacterSet *notLetters = [[NSCharacterSet characterSetWithCharactersInString:@"abcdefghijklmnopqrstuvwxyz"] invertedSet];
    for (NSString *syllable in syllables) {
        if (syllable.length == 0) continue;
        if ([syllable rangeOfCharacterFromSet:notLetters].location != NSNotFound) return nil;
        [codes addObject:syllable];
    }
    if (codes.count != phrase.length) return nil;
    return [codes componentsJoinedByString:@" "];
}
- (BOOL)hasComposition {
    RIME_STRUCT(RimeContext, context);
    BOOL composing = NO;
    if (rime_get_api()->get_context(_session, &context)) {
        composing = context.composition.length > 0;
        rime_get_api()->free_context(&context);
    }
    return composing;
}
- (OLRimeVocabularyUpdate)importConfirmedPhrases:(NSArray<NSString *> *)phrases
                                      revision:(NSString *)revision
                                         error:(NSError **)error {
    NSParameterAssert(NSThread.isMainThread);
    if (revision.length == 0 || revision.length > 128 || phrases.count > 256) {
        if (error) *error = [OLRimeSession vocabularyError:@"确认词典超出边界或存在多个写入会话"];
        return OLRimeVocabularyUpdateFailed;
    }
    NSUInteger bytes = 0;
    for (id phrase in phrases) {
        if (![phrase isKindOfClass:NSString.class] || [phrase length] == 0 || [phrase length] > 128 ||
            [phrase rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound) {
            if (error) *error = [OLRimeSession vocabularyError:@"确认词条格式无效"];
            return OLRimeVocabularyUpdateFailed;
        }
        bytes += [phrase lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
    }
    if (bytes > 64 * 1024) {
        if (error) *error = [OLRimeSession vocabularyError:@"确认词典超出大小限制"];
        return OLRimeVocabularyUpdateFailed;
    }
    if ([_vocabularyRevision isEqualToString:revision] && [_appliedPhrases isEqualToArray:phrases]) {
        // A later snapshot may revoke an earlier deferred update.
        _pendingPhrases = nil; _pendingRevision = nil;
        _vocabularyImportError = nil;
        if (error) *error = nil;
        return OLRimeVocabularyUpdateUnchanged;
    }
    _pendingPhrases = [[NSArray alloc] initWithArray:phrases copyItems:YES];
    _pendingRevision = [revision copy];
    // Accept the newest valid projection even while another session is alive;
    // only deployment is writer-gated. Revocation cannot resurrect older pending data.
    _vocabularyImportError = nil;
    if (error) *error = nil;
    if ([self hasComposition] || liveSessions != 1) return OLRimeVocabularyUpdateDeferred;
    return [self applyPendingVocabulary:error] ? OLRimeVocabularyUpdateApplied : OLRimeVocabularyUpdateFailed;
}
- (void)removeGeneratedFilesExcept:(NSSet<NSString *> *)keep {
    // Only our isolated namespace, never personal *.userdb or packaged inputs.
    NSFileManager *fm = NSFileManager.defaultManager;
    for (NSString *directory in @[_userPath, [_userPath stringByAppendingPathComponent:@"build"]]) {
        for (NSString *name in [fm contentsOfDirectoryAtPath:directory error:nil]) {
            NSString *path = [directory stringByAppendingPathComponent:name];
            if ([name hasPrefix:@"ol_vocabulary_"] && ![keep containsObject:path]) {
                [fm removeItemAtPath:path error:nil];
            }
        }
    }
}
- (BOOL)applyPendingVocabulary:(NSError **)error {
    if (!_pendingPhrases || [self hasComposition]) return YES;
    // Admission can precede deferred deployment. Re-check the single-writer
    // invariant at the actual write and keep the latest pending snapshot.
    if (liveSessions != 1) {
        _vocabularyImportError = [OLRimeSession vocabularyError:@"确认词典等待唯一写入会话"];
        if (error) *error = _vocabularyImportError;
        return NO;
    }
    NSArray<NSString *> *phrases = _pendingPhrases;
    NSString *revision = _pendingRevision;
    _pendingPhrases = nil; _pendingRevision = nil;
    // A unique dictionary identity avoids Rime's process-local dictionary cache.
    NSString *name = [@"ol_vocabulary_" stringByAppendingString:[NSUUID.UUID.UUIDString stringByReplacingOccurrencesOfString:@"-" withString:@""]];
    NSMutableString *dictionary = [[NSString stringWithFormat:@"---\nname: %@\nversion: '1'\nsort: original\nuse_preset_vocabulary: false\n...\n", name] mutableCopy];
    NSMutableSet<NSString *> *seen = [NSMutableSet new];
    for (NSString *phrase in phrases) {
        NSString *code = [OLRimeSession pinyinForPhrase:phrase];
        if (code && ![seen containsObject:phrase]) {
            [dictionary appendFormat:@"%@\t%@\t1000000\n", phrase, code];
            [seen addObject:phrase];
        }
    }
    BOOL success = YES;
    NSMutableSet<NSString *> *keep = [NSMutableSet new];
    NSString *schemaID = @"pinyin_simp";
    if (seen.count) {
        NSString *base = [NSString stringWithContentsOfFile:[_sharedPath stringByAppendingPathComponent:@"pinyin_simp.schema.yaml"] encoding:NSUTF8StringEncoding error:error];
        if (!base || ![base containsString:@"translators: [punct_translator, script_translator]"]) success = NO;
        NSString *schema = [base stringByReplacingOccurrencesOfString:@"schema_id: pinyin_simp" withString:[@"schema_id: " stringByAppendingString:name]];
        schema = [schema stringByReplacingOccurrencesOfString:@"translators: [punct_translator, script_translator]" withString:@"translators: [punct_translator, script_translator, script_translator@base_pinyin]"];
        // Rime deploy_schema compiles only translator/dictionary. Make our
        // isolated dictionary primary; preserve the existing script translator and
        // pinyin_simp personal learning under a named namespace.
        schema = [schema stringByReplacingOccurrencesOfString:@"\ntranslator:\n" withString:@"\nbase_pinyin:\n"];
        schema = [schema stringByAppendingFormat:@"\ntranslator:\n  dictionary: %@\n  enable_user_dict: false\n  enable_sentence: false\n  initial_quality: 100\n", name];
        NSString *dictPath = [_userPath stringByAppendingPathComponent:[name stringByAppendingString:@".dict.yaml"]];
        NSString *schemaPath = [_userPath stringByAppendingPathComponent:[name stringByAppendingString:@".schema.yaml"]];
        if (success) success = [dictionary writeToFile:dictPath atomically:YES encoding:NSUTF8StringEncoding error:error]
            && [schema writeToFile:schemaPath atomically:YES encoding:NSUTF8StringEncoding error:error];
        if (success) {
            try { success = rime_get_api()->deploy_schema(schemaPath.UTF8String); }
            catch (const std::exception &) { success = NO; }
        }
        if (success) {
            schemaID = name;
            [keep addObject:dictPath]; [keep addObject:schemaPath];
            NSString *build = [_userPath stringByAppendingPathComponent:@"build"];
            for (NSString *file in [NSFileManager.defaultManager contentsOfDirectoryAtPath:build error:nil]) {
                if ([file hasPrefix:name]) [keep addObject:[build stringByAppendingPathComponent:file]];
            }
        }
    }
    // No composition is active; destroying this session flushes Rime's own
    // learning normally. We never replace or directly open personal userdb.
    RimeSessionId previous = _session;
    RimeSessionId replacement = rime_get_api()->create_session();
    if (!replacement || !rime_get_api()->select_schema(replacement, schemaID.UTF8String)) {
        success = NO;
        if (replacement) rime_get_api()->destroy_session(replacement);
        rime_get_api()->select_schema(previous, "pinyin_simp");
        [keep removeAllObjects];
    } else {
        _session = replacement;
        rime_get_api()->destroy_session(previous);
    }
    rime_get_api()->set_option(_session, "ascii_mode", False);
    [self removeGeneratedFilesExcept:keep];
    _vocabularyRevision = success ? revision : nil;
    _appliedPhrases = success ? phrases : nil;
    _vocabularyImportError = success ? nil : [OLRimeSession vocabularyError:@"确认词典部署失败，已保留基础拼音输入"];
    if (!success && error && !*error) *error = _vocabularyImportError;
    return success;
}
@end
