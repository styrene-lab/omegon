import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
	computeWeek,
	computeMonth,
	computeQuarter,
	computeRelative,
	resolveRelative,
	computeIso,
	computeEpoch,
	computeTz,
	computeRange,
	computeAll,
} from "./chronos";

/** Create a local-timezone date from YYYY-MM-DD at noon (avoids DST edge) */
function d(ymd: string): Date {
	const [y, m, day] = ymd.split("-").map(Number);
	return new Date(y, m - 1, day, 12, 0, 0);
}

/** Format Date as YYYY-MM-DD */
function fmt(date: Date): string {
	const y = date.getFullYear();
	const m = String(date.getMonth() + 1).padStart(2, "0");
	const day = String(date.getDate()).padStart(2, "0");
	return `${y}-${m}-${day}`;
}

describe("computeWeek", () => {
	it("Wednesday boundaries", () => {
		const out = computeWeek(d("2026-03-18"));
		assert.ok(out.includes("DATE_CONTEXT:"));
		assert.ok(out.includes("TODAY: 2026-03-18 (Wednesday)"));
		assert.ok(out.includes("CURR_WEEK_START: 2026-03-16 (Monday)"));
		assert.ok(out.includes("CURR_WEEK_END: 2026-03-20 (Friday)"));
		assert.ok(out.includes("PREV_WEEK_START: 2026-03-09 (Monday)"));
		assert.ok(out.includes("PREV_WEEK_END: 2026-03-13 (Friday)"));
	});

	it("Monday boundaries", () => {
		const out = computeWeek(d("2026-03-16"));
		assert.ok(out.includes("CURR_WEEK_START: 2026-03-16 (Monday)"));
		assert.ok(out.includes("CURR_WEEK_END: 2026-03-20 (Friday)"));
	});

	it("Friday boundaries", () => {
		const out = computeWeek(d("2026-03-20"));
		assert.ok(out.includes("CURR_WEEK_START: 2026-03-16 (Monday)"));
	});

	it("Sunday belongs to same week as preceding Monday", () => {
		const out = computeWeek(d("2026-03-22"));
		assert.ok(out.includes("CURR_WEEK_START: 2026-03-16 (Monday)"));
	});
});

describe("computeMonth", () => {
	it("March boundaries", () => {
		const out = computeMonth(d("2026-03-15"));
		assert.ok(out.includes("MONTH_CONTEXT:"));
		assert.ok(out.includes("CURR_MONTH_START: 2026-03-01"));
		assert.ok(out.includes("CURR_MONTH_END: 2026-03-31"));
		assert.ok(out.includes("PREV_MONTH_START: 2026-02-01"));
		assert.ok(out.includes("PREV_MONTH_END: 2026-02-28"));
	});

	it("Feb in leap year", () => {
		const out = computeMonth(d("2028-02-15"));
		assert.ok(out.includes("CURR_MONTH_END: 2028-02-29"));
		assert.ok(out.includes("PREV_MONTH_END: 2028-01-31"));
	});

	it("January → Dec rollover", () => {
		const out = computeMonth(d("2026-01-10"));
		assert.ok(out.includes("CURR_MONTH_START: 2026-01-01"));
		assert.ok(out.includes("PREV_MONTH_START: 2025-12-01"));
		assert.ok(out.includes("PREV_MONTH_END: 2025-12-31"));
	});
});

describe("computeQuarter", () => {
	it("Q1 March → FQ2", () => {
		const out = computeQuarter(d("2026-03-15"));
		assert.ok(out.includes("CALENDAR_QUARTER: Q1 2026"));
		assert.ok(out.includes("FISCAL_YEAR: FY2026 (Oct-Sep)"));
		assert.ok(out.includes("FISCAL_QUARTER: FQ2"));
	});

	it("Q2 June → FQ3", () => {
		const out = computeQuarter(d("2026-06-01"));
		assert.ok(out.includes("CALENDAR_QUARTER: Q2 2026"));
		assert.ok(out.includes("FISCAL_QUARTER: FQ3"));
	});

	it("Q3 August → FQ4", () => {
		const out = computeQuarter(d("2026-08-20"));
		assert.ok(out.includes("CALENDAR_QUARTER: Q3 2026"));
		assert.ok(out.includes("FISCAL_QUARTER: FQ4"));
	});

	it("Q4 November → FQ1 of next FY", () => {
		const out = computeQuarter(d("2026-11-15"));
		assert.ok(out.includes("CALENDAR_QUARTER: Q4 2026"));
		assert.ok(out.includes("FISCAL_YEAR: FY2027 (Oct-Sep)"));
		assert.ok(out.includes("FISCAL_QUARTER: FQ1"));
	});
});

describe("resolveRelative", () => {
	const now = d("2026-03-18"); // Wednesday

	it("yesterday", () => assert.equal(fmt(resolveRelative("yesterday", now)), "2026-03-17"));
	it("tomorrow", () => assert.equal(fmt(resolveRelative("tomorrow", now)), "2026-03-19"));
	it("today", () => assert.equal(fmt(resolveRelative("today", now)), "2026-03-18"));

	it("3 days ago", () => assert.equal(fmt(resolveRelative("3 days ago", now)), "2026-03-15"));
	it("1 day ago", () => assert.equal(fmt(resolveRelative("1 day ago", now)), "2026-03-17"));
	it("2 weeks ago", () => assert.equal(fmt(resolveRelative("2 weeks ago", now)), "2026-03-04"));
	it("2 months ago", () => assert.equal(fmt(resolveRelative("2 months ago", now)), "2026-01-18"));
	it("1 month ago", () => assert.equal(fmt(resolveRelative("1 month ago", now)), "2026-02-18"));

	it("5 days from now", () => assert.equal(fmt(resolveRelative("5 days from now", now)), "2026-03-23"));
	it("2 weeks from now", () => assert.equal(fmt(resolveRelative("2 weeks from now", now)), "2026-04-01"));

	it("next Monday", () => assert.equal(fmt(resolveRelative("next Monday", now)), "2026-03-23"));
	it("next Friday", () => assert.equal(fmt(resolveRelative("next Friday", now)), "2026-03-20"));
	it("next Wednesday (same day → +7)", () => assert.equal(fmt(resolveRelative("next Wednesday", now)), "2026-03-25"));
	it("next Sunday", () => assert.equal(fmt(resolveRelative("next Sunday", now)), "2026-03-22"));

	it("last Monday", () => assert.equal(fmt(resolveRelative("last Monday", now)), "2026-03-16"));
	it("last Friday", () => assert.equal(fmt(resolveRelative("last Friday", now)), "2026-03-13"));
	it("last Wednesday (same day → -7)", () => assert.equal(fmt(resolveRelative("last Wednesday", now)), "2026-03-11"));
	it("last Sunday", () => assert.equal(fmt(resolveRelative("last Sunday", now)), "2026-03-15"));

	it("throws on unrecognized expression", () => {
		assert.throws(() => resolveRelative("third Thursday of next month", now), /Cannot parse/);
	});
});

describe("computeRelative", () => {
	it("formats output correctly", () => {
		const out = computeRelative("3 days ago", d("2026-03-18"));
		assert.ok(out.includes("RELATIVE_DATE:"));
		assert.ok(out.includes("EXPRESSION: 3 days ago"));
		assert.ok(out.includes("RESOLVED: 2026-03-15"));
		assert.ok(out.includes("TODAY: 2026-03-18"));
	});
});

describe("computeIso", () => {
	it("ISO week for 2026-03-18", () => {
		const out = computeIso(d("2026-03-18"));
		assert.ok(out.includes("ISO_CONTEXT:"));
		assert.ok(out.includes("ISO_WEEK: W12"));
		assert.ok(out.includes("ISO_YEAR: 2026"));
		assert.ok(out.includes("DAY_OF_YEAR:"));
	});

	it("Jan 1 2026 is W01", () => {
		const out = computeIso(d("2026-01-01"));
		assert.ok(out.includes("ISO_WEEK: W01"));
	});
});

describe("computeEpoch", () => {
	it("returns seconds and millis", () => {
		const now = new Date(1742313600000);
		const out = computeEpoch(now);
		assert.ok(out.includes("EPOCH_CONTEXT:"));
		assert.ok(out.includes("UNIX_SECONDS: 1742313600"));
		assert.ok(out.includes("UNIX_MILLIS: 1742313600000"));
	});
});

describe("computeTz", () => {
	it("returns timezone info", () => {
		const out = computeTz();
		assert.ok(out.includes("TIMEZONE_CONTEXT:"));
		assert.ok(out.includes("TIMEZONE:"));
		assert.ok(out.includes("UTC_OFFSET:"));
	});
});

describe("computeRange", () => {
	it("work week: 4 calendar, 4 business", () => {
		const out = computeRange("2026-03-16", "2026-03-20");
		assert.ok(out.includes("CALENDAR_DAYS: 4"));
		assert.ok(out.includes("BUSINESS_DAYS: 4"));
	});

	it("across weekend: Fri→Wed = 5 cal, 3 biz", () => {
		const out = computeRange("2026-03-13", "2026-03-18");
		assert.ok(out.includes("CALENDAR_DAYS: 5"));
		assert.ok(out.includes("BUSINESS_DAYS: 3"));
	});

	it("throws on invalid date format", () => {
		assert.throws(() => computeRange("not-a-date", "2026-03-20"), /Invalid date format/);
	});

	it("throws on empty date", () => {
		assert.throws(() => computeRange("", "2026-03-20"), /Invalid date format/);
	});
});

describe("computeAll", () => {
	it("contains all section headers", () => {
		const out = computeAll(d("2026-03-18"));
		assert.ok(out.includes("DATE_CONTEXT:"));
		assert.ok(out.includes("MONTH_CONTEXT:"));
		assert.ok(out.includes("QUARTER_CONTEXT:"));
		assert.ok(out.includes("ISO_CONTEXT:"));
		assert.ok(out.includes("EPOCH_CONTEXT:"));
		assert.ok(out.includes("TIMEZONE_CONTEXT:"));
	});
});
