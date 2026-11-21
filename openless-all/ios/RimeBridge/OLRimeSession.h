#import <Foundation/Foundation.h>
NS_ASSUME_NONNULL_BEGIN
@interface OLRimeSnapshot : NSObject
@property(nonatomic, copy, readonly) NSString *preedit;
/// Exact typed input, before syllable separators and preedit substitutions.
@property(nonatomic, copy, readonly) NSString *rawInput;
@property(nonatomic, copy, readonly) NSString *committedText;
@property(nonatomic, copy, readonly) NSArray<NSString *> *candidates;
@property(nonatomic, readonly) BOOL handled;
@property(nonatomic, readonly) BOOL hasNextPage;
@end

/// All methods are confined to the keyboard's main thread. The library has no
/// network access. Packaged input data stays read-only; confirmed phrase text
/// is compiled separately in the extension's private directory.
typedef NS_ENUM(NSInteger, OLRimeVocabularyUpdate) {
    OLRimeVocabularyUpdateFailed = 0,
    OLRimeVocabularyUpdateApplied,
    OLRimeVocabularyUpdateDeferred,
    OLRimeVocabularyUpdateUnchanged,
};
@interface OLRimeSession : NSObject
/// Includes deferred redeploy failures, for parent diagnostic/status presentation.
@property(nonatomic, readonly, nullable) NSError *vocabularyImportError;
- (nullable instancetype)initWithSharedDataPath:(NSString *)shared
                                  userDataPath:(NSString *)user
                                         error:(NSError **)error;
- (OLRimeSnapshot *)processKey:(int)keyCode;
- (OLRimeSnapshot *)selectCandidate:(NSUInteger)index;
- (OLRimeSnapshot *)commitComposition;
/// Commits the original letters without choosing a Chinese candidate.
- (OLRimeSnapshot *)commitRawInput;
- (OLRimeSnapshot *)clearComposition;
- (OLRimeSnapshot *)nextPage;
/// Extension-private single writer; never imports into the personal userdb.
/// Revision must identify the complete immutable snapshot (scope + generation).
/// Active composition is preserved; latest update is applied at the next idle boundary.
- (OLRimeVocabularyUpdate)importConfirmedPhrases:(NSArray<NSString *> *)phrases
                                      revision:(NSString *)revision
                                         error:(NSError * _Nullable * _Nullable)error;
/// Conservative native Mandarin transliteration: pure Han only, or nil.
+ (nullable NSString *)pinyinForPhrase:(NSString *)phrase;
@end
NS_ASSUME_NONNULL_END
