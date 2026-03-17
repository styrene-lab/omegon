/**
 * chronos — Pure TypeScript date/time context functions
 *
 * Replaces chronos.sh. All functions accept an injectable `now` for deterministic testing.
 * Output format matches the original shell script exactly for backward compatibility.
 */

// ── Helpers ──────────────────────────────────────────────────────────────────

const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"] as const;
const MONTHS_SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"] as const;

/** Format as YYYY-MM-DD */
function ymd(d: Date): string {
	const y = d.getFullYear();
	const m = String(d.getMonth() + 1).padStart(2, "0");
	const day = String(d.getDate()).padStart(2, "0");
	return `${y}-${m}-${day}`;
}

/** Day of week name */
function dowName(d: Date): string {
	return DAYS[d.getDay()];
}

/** ISO day of week: 1=Monday … 7=Sunday */
function isoDow(d: Date): number {
	return d.getDay() === 0 ? 7 : d.getDay();
}

/** Format as "Mon D" (e.g. "Mar 18") */
function formatShort(d: Date): string {
	return `${MONTHS_SHORT[d.getMonth()]} ${d.getDate()}`;
}

/** Add N days to a date (returns new Date) */
function addDays(d: Date, n: number): Date {
	const r = new Date(d);
	r.setDate(r.getDate() + n);
	return r;
}

/** Add N months to a date */
function addMonths(d: Date, n: number): Date {
	const r = new Date(d);
	r.setMonth(r.getMonth() + n);
	return r;
}

/** First day of month */
function monthStart(year: number, month: number): Date {
	return new Date(year, month, 1);
}

/** Last day of month */
function monthEnd(year: number, month: number): Date {
	return new Date(year, month + 1, 0);
}

/** Build range string like "Mar 16 - Mar 20, 2026" handling year boundaries */
function weekRange(mon: Date, fri: Date): string {
	const monY = mon.getFullYear();
	const friY = fri.getFullYear();
	if (monY === friY) {
		return `${formatShort(mon)} - ${formatShort(fri)}, ${friY}`;
	}
	return `${formatShort(mon)}, ${monY} - ${formatShort(fri)}, ${friY}`;
}

// ── Subcommands ──────────────────────────────────────────────────────────────

export function computeWeek(now: Date = new Date()): string {
	const dow = isoDow(now);
	const daysSinceMon = dow - 1;
	const currMon = addDays(now, -daysSinceMon);
	const currFri = addDays(currMon, 4);
	const prevMon = addDays(currMon, -7);
	const prevFri = addDays(prevMon, 4);

	return [
		"DATE_CONTEXT:",
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
		`  CURR_WEEK_START: ${ymd(currMon)} (Monday)`,
		`  CURR_WEEK_END: ${ymd(currFri)} (Friday)`,
		`  CURR_WEEK_RANGE: ${weekRange(currMon, currFri)}`,
		`  PREV_WEEK_START: ${ymd(prevMon)} (Monday)`,
		`  PREV_WEEK_END: ${ymd(prevFri)} (Friday)`,
		`  PREV_WEEK_RANGE: ${weekRange(prevMon, prevFri)}`,
	].join("\n");
}

export function computeMonth(now: Date = new Date()): string {
	const year = now.getFullYear();
	const month = now.getMonth();

	const currStart = monthStart(year, month);
	const currEnd = monthEnd(year, month);

	const prevMonth = month === 0 ? 11 : month - 1;
	const prevYear = month === 0 ? year - 1 : year;
	const prevStart = monthStart(prevYear, prevMonth);
	const prevEnd = monthEnd(prevYear, prevMonth);

	return [
		"MONTH_CONTEXT:",
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
		`  CURR_MONTH_START: ${ymd(currStart)}`,
		`  CURR_MONTH_END: ${ymd(currEnd)}`,
		`  CURR_MONTH_RANGE: ${formatShort(currStart)} - ${formatShort(currEnd)}, ${year}`,
		`  PREV_MONTH_START: ${ymd(prevStart)}`,
		`  PREV_MONTH_END: ${ymd(prevEnd)}`,
		`  PREV_MONTH_RANGE: ${formatShort(prevStart)}, ${prevYear} - ${formatShort(prevEnd)}, ${prevEnd.getFullYear()}`,
	].join("\n");
}

export function computeQuarter(now: Date = new Date()): string {
	const year = now.getFullYear();
	const month = now.getMonth() + 1; // 1-based

	const quarter = Math.ceil(month / 3);
	const qStartMonth = (quarter - 1) * 3; // 0-based
	const qStart = monthStart(year, qStartMonth);
	const qEnd = monthEnd(year, qStartMonth + 2);

	let fyYear: number, fyStart: string, fyEnd: string;
	if (month >= 10) {
		fyYear = year + 1;
		fyStart = `${year}-10-01`;
		fyEnd = `${fyYear}-09-30`;
	} else {
		fyYear = year;
		fyStart = `${year - 1}-10-01`;
		fyEnd = `${year}-09-30`;
	}

	const fyMonthOffset = month >= 10 ? month - 10 + 1 : month + 3;
	const fq = Math.ceil(fyMonthOffset / 3);

	return [
		"QUARTER_CONTEXT:",
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
		`  CALENDAR_QUARTER: Q${quarter} ${year}`,
		`  QUARTER_START: ${ymd(qStart)}`,
		`  QUARTER_END: ${ymd(qEnd)}`,
		`  FISCAL_YEAR: FY${fyYear} (Oct-Sep)`,
		`  FISCAL_QUARTER: FQ${fq}`,
		`  FY_START: ${fyStart}`,
		`  FY_END: ${fyEnd}`,
	].join("\n");
}

/** Resolve a relative date expression. Throws on unrecognized expressions. */
export function resolveRelative(expression: string, now: Date = new Date()): Date {
	const expr = expression.trim().toLowerCase();

	if (expr === "yesterday") return addDays(now, -1);
	if (expr === "tomorrow") return addDays(now, 1);
	if (expr === "today") return now;

	// N days/weeks/months ago
	const agoMatch = expr.match(/^(\d+)\s+(day|days|week|weeks|month|months)\s+ago$/);
	if (agoMatch) {
		const n = parseInt(agoMatch[1], 10);
		const unit = agoMatch[2];
		if (unit.startsWith("day")) return addDays(now, -n);
		if (unit.startsWith("week")) return addDays(now, -n * 7);
		if (unit.startsWith("month")) return addMonths(now, -n);
	}

	// N days/weeks from now / ahead / from today
	const aheadMatch = expr.match(/^(\d+)\s+(day|days|week|weeks)\s+(from now|ahead|from today)$/);
	if (aheadMatch) {
		const n = parseInt(aheadMatch[1], 10);
		const unit = aheadMatch[2];
		if (unit.startsWith("day")) return addDays(now, n);
		if (unit.startsWith("week")) return addDays(now, n * 7);
	}

	// next/last {weekday}
	const dayNames = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
	const dayMatch = expr.match(/^(next|last)\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)$/);
	if (dayMatch) {
		const direction = dayMatch[1];
		const targetDow = dayNames.indexOf(dayMatch[2]);
		const currentDow = now.getDay();

		if (direction === "next") {
			let diff = targetDow - currentDow;
			if (diff <= 0) diff += 7;
			return addDays(now, diff);
		} else {
			let diff = currentDow - targetDow;
			if (diff <= 0) diff += 7;
			return addDays(now, -diff);
		}
	}

	throw new Error(`Cannot parse relative expression: '${expression}'. Supported: N days/weeks/months ago, N days/weeks from now, yesterday, tomorrow, next/last {weekday}.`);
}

export function computeRelative(expression: string, now: Date = new Date()): string {
	const resolved = resolveRelative(expression, now);
	return [
		"RELATIVE_DATE:",
		`  EXPRESSION: ${expression}`,
		`  RESOLVED: ${ymd(resolved)} (${dowName(resolved)})`,
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
	].join("\n");
}

/** ISO 8601 week number (Thursday-based) */
function isoWeekNumber(d: Date): { week: number; year: number } {
	const tmp = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
	const dayNum = tmp.getUTCDay() || 7;
	tmp.setUTCDate(tmp.getUTCDate() + 4 - dayNum);
	const yearStart = new Date(Date.UTC(tmp.getUTCFullYear(), 0, 1));
	const week = Math.ceil(((tmp.getTime() - yearStart.getTime()) / 86400000 + 1) / 7);
	return { week, year: tmp.getUTCFullYear() };
}

/** Day of year (1-366) */
function dayOfYear(d: Date): number {
	const start = new Date(d.getFullYear(), 0, 0);
	const diff = d.getTime() - start.getTime();
	return Math.floor(diff / 86400000);
}

export function computeIso(now: Date = new Date()): string {
	const { week, year } = isoWeekNumber(now);
	const wStr = String(week).padStart(2, "0");
	const doy = String(dayOfYear(now)).padStart(3, "0");
	const dow = isoDow(now);

	return [
		"ISO_CONTEXT:",
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
		`  ISO_WEEK: W${wStr}`,
		`  ISO_YEAR: ${year}`,
		`  ISO_WEEKDATE: ${year}-W${wStr}-${dow}`,
		`  DAY_OF_YEAR: ${doy}`,
	].join("\n");
}

export function computeEpoch(now: Date = new Date()): string {
	const seconds = Math.floor(now.getTime() / 1000);
	const millis = now.getTime();

	return [
		"EPOCH_CONTEXT:",
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
		`  UNIX_SECONDS: ${seconds}`,
		`  UNIX_MILLIS: ${millis}`,
	].join("\n");
}

export function computeTz(now: Date = new Date()): string {
	const tzParts = now.toTimeString().match(/\((.+)\)/);
	const tzAbbrev = tzParts
		? tzParts[1].replace(/[a-z ]/g, "") || tzParts[1]
		: Intl.DateTimeFormat(undefined, { timeZoneName: "short" }).formatToParts(now).find(p => p.type === "timeZoneName")?.value || "Unknown";

	const offsetMin = now.getTimezoneOffset();
	const sign = offsetMin <= 0 ? "+" : "-";
	const absMin = Math.abs(offsetMin);
	const hh = String(Math.floor(absMin / 60)).padStart(2, "0");
	const mm = String(absMin % 60).padStart(2, "0");
	const utcOffset = `${sign}${hh}${mm}`;

	return [
		"TIMEZONE_CONTEXT:",
		`  TODAY: ${ymd(now)} (${dowName(now)})`,
		`  TIMEZONE: ${tzAbbrev}`,
		`  UTC_OFFSET: ${utcOffset}`,
	].join("\n");
}

export function computeRange(fromDate: string, toDate: string): string {
	const dateRe = /^\d{4}-\d{2}-\d{2}$/;
	if (!dateRe.test(fromDate)) throw new Error(`Invalid date format '${fromDate}'. Use YYYY-MM-DD.`);
	if (!dateRe.test(toDate)) throw new Error(`Invalid date format '${toDate}'. Use YYYY-MM-DD.`);

	const d1 = new Date(fromDate + "T00:00:00");
	const d2 = new Date(toDate + "T00:00:00");

	if (isNaN(d1.getTime())) throw new Error(`Could not parse date: ${fromDate}`);
	if (isNaN(d2.getTime())) throw new Error(`Could not parse date: ${toDate}`);

	const diffMs = d2.getTime() - d1.getTime();
	const calendarDays = Math.round(diffMs / 86400000);
	const absDays = Math.abs(calendarDays);

	let businessDays = 0;
	const step = calendarDays >= 0 ? 1 : -1;
	let cursor = new Date(d1);
	for (let i = 0; i < absDays; i++) {
		const dow = cursor.getDay();
		if (dow >= 1 && dow <= 5) businessDays++;
		cursor = addDays(cursor, step);
	}

	return [
		"RANGE_CONTEXT:",
		`  FROM: ${fromDate}`,
		`  TO: ${toDate}`,
		`  CALENDAR_DAYS: ${absDays}`,
		`  BUSINESS_DAYS: ${businessDays}`,
	].join("\n");
}

export function computeAll(now: Date = new Date()): string {
	return [
		computeWeek(now),
		"",
		computeMonth(now),
		"",
		computeQuarter(now),
		"",
		computeIso(now),
		"",
		computeEpoch(now),
		"",
		computeTz(now),
	].join("\n");
}
