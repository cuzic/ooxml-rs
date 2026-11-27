const millisecondsPerDay = 86400000;

export function serialFromDate(milliseconds, date1904) {
  if (date1904) return (milliseconds - Date.UTC(1904, 0, 1)) / millisecondsPerDay;
  const days = (milliseconds - Date.UTC(1899, 11, 31)) / millisecondsPerDay;
  return days < 60 ? days : days + 1;
}

export function displaySerialFromDate(milliseconds, date1904) {
  const serial = serialFromDate(milliseconds, date1904);
  // SheetJS SSF maps February 28, 1900 to the fictitious leap day.
  return !date1904 && serial >= 59 && serial < 60 ? serial + 1 : serial;
}

export function dateFromSerial(serial, date1904) {
  if (!date1904 && serial >= 60 && serial < 61) return undefined;
  const days = date1904 ? serial + 1462 : serial < 60 ? serial + 1 : serial;
  const millis = Math.round((days - 25569) * millisecondsPerDay);
  const date = new Date(millis);
  return Number.isFinite(date.getTime()) ? date : undefined;
}

function roundClockMillis(millis, fraction) {
  const unit = fraction ? 10 ** (4 - fraction.length) : 1000;
  return Math.round(millis / unit) * unit;
}

function timeText(value, format) {
  const match = /^(h{1,2})(?::(m{1,2})(?::(s{1,2})(\.0{1,3})?)?)?(?:( ?)(AM\/PM|A\/P))?$/i.exec(format);
  if (!match) return undefined;
  let millis;
  if (value instanceof Date) {
    const timestamp = value.getTime();
    if (!Number.isFinite(timestamp)) return '';
    millis = (timestamp % millisecondsPerDay + millisecondsPerDay) % millisecondsPerDay;
  } else if (typeof value === 'number') {
    if (!Number.isFinite(value) || value < 0) return '';
    millis = (value % 1) * millisecondsPerDay;
  } else return undefined;

  const [, h, m, s, fraction, periodSpace, period] = match;
  const afternoon = millis >= millisecondsPerDay / 2;
  millis = roundClockMillis(millis, fraction) % millisecondsPerDay;
  const hours = Math.floor(millis / 3600000);
  const minutes = Math.floor(millis / 60000) % 60;
  const seconds = Math.floor(millis / 1000) % 60;
  let text = String(period ? hours % 12 || 12 : hours).padStart(h.length, '0');
  if (m) text += ':' + String(minutes).padStart(m.length, '0');
  if (s) text += ':' + String(seconds).padStart(s.length, '0');
  if (fraction) text += '.' + String(millis % 1000).padStart(3, '0').slice(0, fraction.length - 1);
  if (period) {
    text += periodSpace + (period.length === 3 ? period[afternoon ? 2 : 0] : afternoon ? 'PM' : 'AM');
  }
  return text;
}

const months = ['January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December'];
const weekdays = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const padded = (number, digits = 2) => String(number).padStart(digits, '0');

function durationText(value, format, date1904) {
  const elapsed = /^\[(h{1,2}|m{1,2}|s{1,2})\](:m{1,2}:s{1,2}|:m{1,2}|:s{1,2})?(\.0{1,3})?$/i.exec(format);
  const minuteSecond = /^(m{1,2}:s{1,2}|mmss(?=\.0$)|ss|s$)(\.0{1,3})?$/i.exec(format);
  if (!elapsed && !minuteSecond) return undefined;
  const serial = value instanceof Date ? serialFromDate(value.getTime(), date1904) : value;
  if (!Number.isFinite(serial) || serial < 0) return '';
  const fraction = elapsed ? elapsed[3] : minuteSecond[2];
  const millis = roundClockMillis(serial * millisecondsPerDay, fraction);
  const seconds = Math.floor(millis / 1000);
  const minutesWithinHour = Math.floor(seconds / 60) % 60;
  const secondsWithinMinute = seconds % 60;
  const displayUnits = pattern => pattern.replace(/m{1,2}|s{1,2}/gi, token =>
    padded(token.toLowerCase().startsWith('m') ? minutesWithinHour : secondsWithinMinute, token.length));
  const decimal = fraction ? '.' + padded(millis % 1000, 3).slice(0, fraction.length - 1) : '';
  if (elapsed) {
    const [, field, suffix = ''] = elapsed;
    const unit = { h: 3600, m: 60, s: 1 }[field[0].toLowerCase()];
    const total = padded(Math.floor(seconds / unit), field.length);
    return total + displayUnits(suffix) + decimal;
  }
  return displayUnits(minuteSecond[1]) + decimal;
}

export function dateText(value, format, date1904) {
  if (format?.includes('[$')) {
    // Locale directives do not contribute characters to a date or time display.
    format = format.replace(/"[^"]*"|\\.|\[\$[^\]\-]*(?:-[0-9a-f]+)?\]/gi,
      token => token.startsWith('[$') ? '' : token);
  }
  if (format === '"上午/下午 "hh"時"mm"分"ss"秒 "') {
    const time = timeText(value, 'hh:mm:ss');
    if (!time) return time;
    const [hours, minutes, seconds] = time.split(':');
    return '上午/下午 ' + hours + '時' + minutes + '分' + seconds + '秒 ';
  }
  const time = timeText(value, format);
  if (time !== undefined) return time;
  const duration = durationText(value, format, date1904);
  if (duration !== undefined) return duration;
  const [calendar, clock, fraction, suffix] =
    /^(.*?)(h{1,2}:m{1,2}(?::s{1,2}(\.0{1,3})?)?(?: ?(?:AM\/PM|A\/P))?)((?:"[^"]*"|\\.)*)$/i
      .exec(format)?.slice(1) ?? [format];
  if (!(value instanceof Date) && typeof value !== 'number') return undefined;
  if (!/^(?:(?:yyyy|yy)(?!y)|m{1,5}(?!m)|d{1,4}(?!d)|"[^"]*"|\\.|[ /.,-])+$/i.test(calendar)) return undefined;
  const calendarFields = /["\\]/.test(calendar) ? calendar.replace(/"[^"]*"|\\./g, '') : calendar;
  if (!/[ymd]/i.test(calendarFields)) return undefined;
  // SSF parses an unescaped period as a numeric token, even in a calendar format.
  if (calendarFields.includes('.')) return null;
  const epoch = date1904 ? Date.UTC(1904, 0, 1) : Date.UTC(1899, 11, 31);
  const serial = value instanceof Date ? serialFromDate(value.getTime(), date1904) : value;
  const rollover = clock && roundClockMillis((serial % 1) * millisecondsPerDay, fraction) === millisecondsPerDay;
  const dayNumber = Math.floor(serial) + Number(Boolean(rollover));
  const date = value instanceof Date
    ? new Date(value.getTime() + (rollover ? millisecondsPerDay : 0)) :
    new Date(epoch + (!date1904 && dayNumber > 60 ? dayNumber - 1 : dayNumber) * millisecondsPerDay);
  if (!Number.isFinite(date.getTime()) || (!(value instanceof Date) && value < 0)) return '';
  const special = !(value instanceof Date) && !date1904 && (dayNumber === 0 || dayNumber === 60);
  const year = special ? 1900 : date.getUTCFullYear();
  const month = special ? (dayNumber === 0 ? 1 : 2) : date.getUTCMonth() + 1;
  const day = special ? (dayNumber === 0 ? 0 : 29) : date.getUTCDate();
  const weekday = special ? (dayNumber === 0 ? 6 : 3) :
    (date.getUTCDay() + (!(value instanceof Date) && !date1904 && dayNumber < 60 ? 6 : 0)) % 7;
  const monthName = months[month - 1];
  const weekdayName = weekdays[weekday];
  const parts = { yyyy: padded(year, 4), yy: padded(year % 100),
    mmmmm: monthName[0], mmmm: monthName, mmm: monthName.slice(0, 3), mm: padded(month), m: month,
    dddd: weekdayName, ddd: weekdayName.slice(0, 3), dd: padded(day), d: day };
  const text = calendar.replace(/"[^"]*"|\\.|yyyy|yy|m{1,5}|d{1,4}/gi, token => {
    if (token.startsWith('"')) return token.slice(1, -1);
    if (token.startsWith('\\')) return token.slice(1);
    return parts[token.toLowerCase()];
  });
  if (!clock) return text;
  const trailingText = suffix.replace(/"([^"]*)"|\\(.)/g, (_, quoted, escaped) => quoted ?? escaped);
  return text + timeText(value, clock) + trailingText;
}

export function dateFromIso(text, date1904) {
  if (text === undefined) return undefined;
  const timeOnly = /^(?:[01]\d|2[0-3]):[0-5]\d(?::[0-5]\d(?:\.\d+)?)?$/.test(text);
  const timestamp = timeOnly ? `${date1904 ? '1904-01-01' : '1899-12-31'}T${text}Z` : text;
  const date = new Date(timestamp);
  return Number.isFinite(date.getTime()) ? date : undefined;
}

export function isoCellText(date) {
  const iso = Date.prototype.toISOString.call(date);
  return iso.startsWith('1899-12-31T') ? iso.slice(11, -1) : iso;
}

