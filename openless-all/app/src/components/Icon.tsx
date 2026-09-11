// Shared interface icons use one 24px grid and a consistent rounded stroke.
import type { CSSProperties } from 'react';
import {
  Archive,
  ChartNoAxesColumn,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleHelp,
  Clock3,
  Cloud,
  CodeXml,
  Command,
  Copy,
  CornerDownLeft,
  Download,
  Ellipsis,
  ExternalLink,
  Eye,
  Feather,
  FileText,
  Hash,
  History,
  Info,
  Languages,
  Link,
  ListFilter,
  Mail,
  Maximize2,
  MessageSquareText,
  Mic,
  Minimize2,
  Minus,
  Monitor,
  Option,
  PanelLeft,
  PanelsTopLeft,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Square,
  Tag,
  Trash2,
  Upload,
  UserRound,
  X,
  Zap,
  BookOpenText,
  type LucideIcon,
} from 'lucide-react';

export const ICONS: Record<string, LucideIcon> = {
  overview: ChartNoAxesColumn,
  history: History,
  vocab: BookOpenText,
  style: SlidersHorizontal,
  translate: Languages,
  selectionAsk: MessageSquareText,
  settings: Settings,
  help: CircleHelp,
  mic: Mic,
  search: Search,
  plus: Plus,
  check: Check,
  x: X,
  copy: Copy,
  eye: Eye,
  trash: Trash2,
  refresh: RefreshCw,
  sparkle: Sparkles,
  bolt: Zap,
  clock: Clock3,
  hash: Hash,
  chevDown: ChevronDown,
  chevRight: ChevronRight,
  chevLeft: ChevronLeft,
  chevLR: CodeXml,
  collapse: Minimize2,
  expand: Maximize2,
  layout: PanelLeft,
  cmd: Command,
  option: Option,
  esc: CornerDownLeft,
  enter: CornerDownLeft,
  inserted: Check,
  cloud: Cloud,
  mac: Monitor,
  win: PanelsTopLeft,
  doc: FileText,
  link: Link,
  filter: ListFilter,
  archive: Archive,
  tag: Tag,
  user: UserRound,
  mail: Mail,
  info: Info,
  shield: ShieldCheck,
  external: ExternalLink,
  close: X,
  more: Ellipsis,
  play: Play,
  download: Download,
  upload: Upload,
  pencil: Pencil,
  feather: Feather,
  minimize: Minus,
  maximize: Square,
  restore: Copy,
};

export interface IconProps {
  name: string;
  size?: number;
  stroke?: string;
  strokeWidth?: number;
  fill?: string;
  style?: CSSProperties;
  className?: string;
}

export function Icon({
  name,
  size = 16,
  stroke = 'currentColor',
  strokeWidth = 1.75,
  fill = 'none',
  style,
  className,
}: IconProps) {
  const Glyph = ICONS[name];
  if (!Glyph) return null;
  return (
    <Glyph
      size={size}
      stroke={stroke}
      strokeWidth={strokeWidth}
      fill={fill}
      style={{ flexShrink: 0, ...style }}
      className={className}
      aria-hidden="true"
      focusable="false"
    />
  );
}
