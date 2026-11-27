/** Locale-aware formatting for the native pages. */

const MONTHS_ZH: string[] = ['1月', '2月', '3月', '4月', '5月', '6月', '7月', '8月', '9月', '10月', '11月', '12月'];

function pad(value: number): string {
  return value < 10 ? '0' + value.toString() : value.toString();
}

export class Fmt {
  /** "刚刚" / "5 分钟前" / "昨天 14:32" / "10月21日 14:32" */
  static relative(iso: string): string {
    if (!iso) return '';
    const then = new Date(iso);
    const ms = then.getTime();
    if (Number.isNaN(ms)) return iso;
    const now = Date.now();
    const diff = Math.max(0, now - ms);
    const minute = 60 * 1000;
    const hour = 60 * minute;
    if (diff < minute) return '刚刚';
    if (diff < hour) return Math.floor(diff / minute).toString() + ' 分钟前';
    const clock = pad(then.getHours()) + ':' + pad(then.getMinutes());
    const today = new Date(now);
    const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
    if (ms >= startOfToday) {
      if (diff < 6 * hour) return Math.floor(diff / hour).toString() + ' 小时前';
      return '今天 ' + clock;
    }
    if (ms >= startOfToday - 24 * hour) return '昨天 ' + clock;
    const sameYear = then.getFullYear() === today.getFullYear();
    const head = (then.getMonth() + 1).toString() + '月' + then.getDate().toString() + '日';
    return sameYear ? head + ' ' + clock : then.getFullYear().toString() + '年' + head;
  }

  /** "0:42" / "1:05:09" */
  static duration(ms: number | null | undefined): string {
    if (ms === null || ms === undefined || ms <= 0) return '—';
    const total = Math.round(ms / 1000);
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor((total % 3600) / 60);
    const seconds = total % 60;
    if (hours > 0) return hours.toString() + ':' + pad(minutes) + ':' + pad(seconds);
    return minutes.toString() + ':' + pad(seconds);
  }

  /** 1234 -> "1,234" */
  static count(value: number): string {
    const text = Math.round(value).toString();
    let out = '';
    let digits = 0;
    for (let index = text.length - 1; index >= 0; index--) {
      out = text.charAt(index) + out;
      digits++;
      if (digits % 3 === 0 && index > 0) out = ',' + out;
    }
    return out;
  }

  static bytes(value: number): string {
    if (value < 1024) return value.toString() + ' B';
    if (value < 1024 * 1024) return (value / 1024).toFixed(1) + ' KB';
    return (value / (1024 * 1024)).toFixed(1) + ' MB';
  }

  static monthDay(isoDate: string): string {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return isoDate;
    return (date.getMonth() + 1).toString() + '月' + date.getDate().toString() + '日';
  }

  static weekday(isoDate: string): string {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return '';
    const names: string[] = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];
    return names[date.getDay()];
  }

  /** Trims a transcript down to one list row. */
  static preview(text: string, limit: number): string {
    if (!text) return '';
    const flat = text.replace(/\s+/g, ' ').trim();
    if (flat.length <= limit) return flat;
    return flat.substring(0, limit) + '…';
  }

  static clock(iso: string): string {
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return '';
    return pad(date.getHours()) + ':' + pad(date.getMinutes());
  }

  static todayIso(): string {
    const now = new Date();
    return now.getFullYear().toString() + '-' + pad(now.getMonth() + 1) + '-' + pad(now.getDate());
  }

  static monthLabel(month: number): string {
    const index = Math.min(Math.max(month - 1, 0), 11);
    return MONTHS_ZH[index];
  }
}
