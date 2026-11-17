import { invokeOrMock } from './shared';

export interface StableExpressionDirection {
  feature: 'lineBreaks' | 'exclamations' | 'terminalPunctuation';
  direction: -1 | 1;
}

export function getExpressionProfile(): Promise<StableExpressionDirection[]> {
  return invokeOrMock('get_expression_profile', undefined, () => []);
}

export function clearExpressionProfile(): Promise<void> {
  return invokeOrMock('clear_expression_profile', undefined, () => undefined);
}
