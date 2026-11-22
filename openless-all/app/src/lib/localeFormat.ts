/** Display formatters always receive the selected UI locale, not the host default. */
const decimalFormats = new Map<string, Intl.NumberFormat>();
const historyTimeFormats = new Map<string, Intl.DateTimeFormat>();

export function formatLocaleNumber(
  value: number,
  locale: string,
  options: Intl.NumberFormatOptions = {},
): string {
  return new Intl.NumberFormat(locale, options).format(value);
}

export function formatLocaleDecimal(value: number, locale: string, digits = 1): string {
  const key = `${locale}:${digits}`;
  let format = decimalFormats.get(key);
  if (!format) {
    format = new Intl.NumberFormat(locale, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
    decimalFormats.set(key, format);
  }
  return format.format(value);
}

function displayDate(value: string): Date {
  const calendar = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  // Activity keys are local calendar dates, not UTC timestamps. Parsing them as
  // UTC shifts the displayed day in time zones west of Greenwich.
  return calendar
    ? new Date(Number(calendar[1]), Number(calendar[2]) - 1, Number(calendar[3]))
    : new Date(value);
}

export function formatLocaleDate(
  value: string,
  locale: string,
  options: Intl.DateTimeFormatOptions = { month: 'numeric', day: 'numeric' },
): string {
  const date = displayDate(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat(locale, options).format(date);
}

export function formatHistoryTime(value: string, locale: string, now = new Date()): string {
  const date = displayDate(value);
  if (Number.isNaN(date.getTime())) return value;
  const sameDay = date.toDateString() === now.toDateString();
  const sameYear = date.getFullYear() === now.getFullYear();
  const key = `${locale}:${sameDay ? 'time' : sameYear ? 'date' : 'year'}`;
  let format = historyTimeFormats.get(key);
  if (!format) {
    const options: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit' };
    if (!sameDay) {
      options.month = 'numeric';
      options.day = 'numeric';
      if (!sameYear) options.year = 'numeric';
    }
    format = new Intl.DateTimeFormat(locale, options);
    historyTimeFormats.set(key, format);
  }
  return format.format(date);
}
