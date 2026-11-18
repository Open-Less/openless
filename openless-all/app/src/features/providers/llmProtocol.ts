import type { LlmRequestFormat } from '../../api/providers';

export const protocolAccounts = [
  'ark.request_format',
  'ark.messages_thinking',
  'ark.max_tokens',
  'ark.thinking_budget',
] as const;
export const serviceTierAccount = 'ark.service_tier' as const;
export type Account = (typeof protocolAccounts)[number] | typeof serviceTierAccount;
export type ProtocolValues = Record<Account, string>;
export const emptyValues: ProtocolValues = {
  'ark.request_format': '',
  'ark.messages_thinking': '',
  'ark.max_tokens': '',
  'ark.thinking_budget': '',
  'ark.service_tier': '',
};

/** Immediate UI hint; Core still validates the same rules for what is stored and sent. */
export function protocolValidationError(
  values: ProtocolValues,
  defaultFormat: LlmRequestFormat,
): string | null {
  const format = values['ark.request_format'] || defaultFormat;
  if (values['ark.service_tier'] && values['ark.service_tier'] !== 'fast') {
    return 'llmServiceTierInvalid';
  }
  if (!['chat_completions', 'responses', 'messages'].includes(format))
    return 'llmRequestFormatInvalid';
  const mode = values['ark.messages_thinking'] || 'adaptive';
  if (!['adaptive', 'budget'].includes(mode)) return 'llmThinkingModeInvalid';
  const max = values['ark.max_tokens'] || '8192';
  const budget = values['ark.thinking_budget'] || '1024';
  if (
    ![max, budget].every(
      (value) => /^\d+$/.test(value) && Number(value) > 0 && Number(value) <= 4294967295,
    )
  )
    return 'llmTokenLimitInvalid';
  if (
    Number(budget) < 1024 ||
    (format === 'messages' && mode === 'budget' && Number(budget) >= Number(max))
  )
    return 'llmThinkingBudgetInvalid';
  return null;
}
