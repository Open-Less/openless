/**
 * Iconography.
 *
 * HarmonyOS ships a system symbol library (`sys.symbol.*`) that follows the
 * platform stroke weight and optical size. Most controls use those symbols;
 * the product dock uses four original, monochrome SVGs where the system glyph
 * changes the meaning or silhouette of the iOS reference.
 */
/** Original product navigation silhouettes, not extracted SF Symbols assets.
 * Huawei Image.fillColor supports monochrome SVG resources.
 * https://developer.huawei.com/consumer/cn/doc/harmonyos-guides-V13/ui-dark-light-color-adaptation-V13
 */
export function hasDockImage(id: string): boolean {
  return id === 'overview' || id === 'vocab' || id === 'settings' || id === 'style';
}

export function dockImageOf(id: string): Resource {
  if (id === 'overview') return $r('app.media.dock_overview');
  if (id === 'vocab') return $r('app.media.dock_dictionary');
  if (id === 'settings') return $r('app.media.dock_settings');
  return $r('app.media.dock_style');
}

export function symbolOf(id: string): Resource {
  if (id === 'overview') return $r('sys.symbol.house');
  if (id === 'history') return $r('sys.symbol.clock');
  if (id === 'vocab') return $r('sys.symbol.character_book');
  if (id === 'style') return $r('sys.symbol.wand_and_stars');
  if (id === 'marketplace') return $r('sys.symbol.square_grid_2x2');
  if (id === 'translation') return $r('sys.symbol.translate');
  if (id === 'qa') return $r('sys.symbol.message');
  if (id === 'quicknote') return $r('sys.symbol.doc_plaintext');
  if (id === 'corrections') return $r('sys.symbol.wand_and_stars');
  if (id === 'localasr') return $r('sys.symbol.waveform');
  if (id === 'settings') return $r('sys.symbol.slider_horizontal_2');
  if (id === 'keyboard') return $r('sys.symbol.keyboard_badge_gearshape');
  if (id === 'mic') return $r('sys.symbol.mic');
  if (id === 'search') return $r('sys.symbol.magnifyingglass');
  if (id === 'close') return $r('sys.symbol.xmark');
  if (id === 'check') return $r('sys.symbol.checkmark');
  if (id === 'chevron') return $r('sys.symbol.chevron_right');
  if (id === 'chevron_left') return $r('sys.symbol.chevron_left');
  if (id === 'chevron_down') return $r('sys.symbol.chevron_down');
  if (id === 'list') return $r('sys.symbol.list_bullet');
  if (id === 'trash') return $r('sys.symbol.trash');
  if (id === 'add') return $r('sys.symbol.plus');
  if (id === 'refresh') return $r('sys.symbol.arrow_clockwise');
  if (id === 'copy') return $r('sys.symbol.doc_plaintext');
  if (id === 'play') return $r('sys.symbol.play_fill');
  if (id === 'more') return $r('sys.symbol.ellipsis_message');
  if (id === 'stop') return $r('sys.symbol.square_fill');
  if (id === 'sparkles') return $r('sys.symbol.star');
  if (id === 'person') return $r('sys.symbol.person');
  return $r('sys.symbol.circle');
}
