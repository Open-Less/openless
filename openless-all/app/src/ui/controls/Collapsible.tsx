import { useState, type ReactNode } from 'react';
import { Icon } from '../../components/Icon';

interface CollapsibleProps {
  /// Title row content (short text, left-aligned).
  title: ReactNode;
  /// Optional subtitle / description (small text below the title).
  desc?: ReactNode;
  /// Whether expanded by default. Default false (collapsed, matching the "title + right arrow only" default).
  defaultOpen?: boolean;
  /// Set true when nested in a Card padding=0 container: removes top/bottom margins, relying only on the Card's borderBottom to separate.
  embedded?: boolean;
  children: ReactNode;
}

/// Collapsible section: collapsed by default, a `›` arrow on the right of the title row toggles
/// expand/collapse. The arrow rotates 90° when expanded. The content area transitions via
/// `grid-template-rows: 0fr ↔ 1fr` — the browser resolves `1fr` to the content's actual height and
/// transitions to it, avoiding the janky feel of a max-height animation where short content still plays
/// the full animation / closes with a delay. Requires Chromium 117+ (bundled with Tauri); modern
/// versions are fully supported.
///
/// `embedded=true`: for nesting inside a `<Card padding={0}>` sharing one Card with other Collapsibles;
/// adds a 0.5px bottom separator.
/// `embedded=false`: standalone block with the Card look (border / radius / shadow).
export function Collapsible({
  title,
  desc,
  defaultOpen = false,
  embedded = false,
  children,
}: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div
      data-ol-toggle-group=""
      style={{
        borderBottom: embedded ? '0.5px solid var(--ol-line)' : undefined,
        border: embedded ? undefined : '0.5px solid var(--ol-line)',
        borderRadius: embedded ? 0 : 'var(--ol-r-lg)',
        background: embedded ? 'transparent' : 'var(--ol-surface)',
        boxShadow: embedded ? 'none' : 'var(--ol-shadow-sm)',
        overflow: 'hidden',
        // In a parent flex column with minHeight:0 + overflow:auto, flex children default to shrink:1
        // and the header button would be squeezed to a line. Lock it from shrinking; overflow scrolls in the parent.
        flexShrink: 0,
      }}
    >
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        // Announce the collapsed state to screen readers; keyboard focus keeps the browser's default outline
        // instead of outline: 'none' so Tab navigation keeps a visual focus indicator (pr-agent #407).
        style={{
          width: '100%',
          padding: '14px 18px',
          background: 'transparent',
          border: 0,
          textAlign: 'left',
          fontFamily: 'inherit',
          color: 'inherit',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          cursor: 'pointer',
        }}
      >
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ fontSize: 13, fontWeight: 'var(--ol-font-weight-semibold)' }}>{title}</div>
          {desc && (
            <div
              style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 3, lineHeight: 1.5 }}
            >
              {desc}
            </div>
          )}
        </div>
        <span
          aria-hidden="true"
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            width: 18,
            height: 18,
            color: 'var(--ol-ink-4)',
            transform: open ? 'rotate(90deg)' : 'rotate(0deg)',
            transition: 'transform 0.18s var(--ol-motion-quick)',
          }}
        >
          <Icon name="chevRight" size={14} />
        </span>
      </button>
      <div
        style={{
          display: 'grid',
          // grid-template-rows: 0fr → 1fr makes the browser resolve 1fr to the content's actual height.
          gridTemplateRows: open ? '1fr' : '0fr',
          transition: 'grid-template-rows 0.22s var(--ol-motion-soft)',
        }}
        // inert removes internal interactive elements from tab order + the a11y tree so keyboard users
        // can't tab into invisible inputs / buttons / toggles after collapsing (pr-agent #407).
        // Support: Chromium 102+ (the Tauri WebView is far newer) / Safari 15.4+.
        // React 18 types don't accept `inert`; pass the string-boolean via spread to bypass the compiler.
        {...(!open ? { inert: '' } : {})}
        aria-hidden={!open}
      >
        {/* minHeight: 0 is required: a grid item won't shrink below its content's intrinsic height by default;
            without this trick nothing animates and the row-height animation fails too. */}
        <div style={{ overflow: 'hidden', minHeight: 0 }}>
          <div style={{ padding: '0 18px 18px' }}>{children}</div>
        </div>
      </div>
    </div>
  );
}
