#!/bin/bash
# chronos.sh - Comprehensive date and time context
# Outputs structured date context to eliminate AI date calculation errors
# Cross-platform: macOS (BSD date) and Linux (GNU date)

set -uo pipefail

# Detect date command flavor
if date --version >/dev/null 2>&1; then
  DATE_FLAVOR="gnu"
else
  DATE_FLAVOR="bsd"
fi

# Helper: add/subtract days from a date
# Usage: date_add "2026-01-25" -3  -> outputs date 3 days earlier
date_add() {
  local base_date=$1
  local days=$2
  if [[ "$DATE_FLAVOR" == "gnu" ]]; then
    date -d "$base_date $days days" "+%Y-%m-%d"
  else
    # BSD date: convert to epoch, add seconds, convert back
    local epoch=$(date -j -f "%Y-%m-%d" "$base_date" "+%s" 2>/dev/null)
    local new_epoch=$((epoch + days * 86400))
    date -r "$new_epoch" "+%Y-%m-%d"
  fi
}

# Helper: format date as "Mon D"
format_short() {
  local ymd=$1
  local month=$(echo "$ymd" | cut -d'-' -f2)
  local day=$(echo "$ymd" | cut -d'-' -f3 | sed 's/^0//')
  case $month in
    01) mon="Jan" ;; 02) mon="Feb" ;; 03) mon="Mar" ;;
    04) mon="Apr" ;; 05) mon="May" ;; 06) mon="Jun" ;;
    07) mon="Jul" ;; 08) mon="Aug" ;; 09) mon="Sep" ;;
    10) mon="Oct" ;; 11) mon="Nov" ;; 12) mon="Dec" ;;
  esac
  echo "$mon $day"
}

# Helper: get epoch from date string
date_to_epoch() {
  local ymd=$1
  if [[ "$DATE_FLAVOR" == "gnu" ]]; then
    date -d "$ymd" "+%s"
  else
    date -j -f "%Y-%m-%d" "$ymd" "+%s" 2>/dev/null
  fi
}

# Helper: resolve a relative date expression
resolve_relative() {
  local expr=$1
  if [[ "$DATE_FLAVOR" == "gnu" ]]; then
    date -d "$expr" "+%Y-%m-%d" 2>/dev/null
  else
    # BSD: handle common expressions manually
    case "$expr" in
      *"days ago")
        local n=$(echo "$expr" | grep -oE '[0-9]+')
        date_add "$TODAY" "-$n"
        ;;
      *"days from now"|*"days ahead")
        local n=$(echo "$expr" | grep -oE '[0-9]+')
        date_add "$TODAY" "$n"
        ;;
      "yesterday")
        date_add "$TODAY" "-1"
        ;;
      "tomorrow")
        date_add "$TODAY" "1"
        ;;
      "next Monday"|"next monday")
        local days_ahead=$(( (8 - DOW_NUM) % 7 ))
        [[ $days_ahead -eq 0 ]] && days_ahead=7
        date_add "$TODAY" "$days_ahead"
        ;;
      "next Friday"|"next friday")
        local target=5
        local days_ahead=$(( (target - DOW_NUM + 7) % 7 ))
        [[ $days_ahead -eq 0 ]] && days_ahead=7
        date_add "$TODAY" "$days_ahead"
        ;;
      "last Monday"|"last monday")
        local days_back=$(( (DOW_NUM - 1 + 7) % 7 ))
        [[ $days_back -eq 0 ]] && days_back=7
        date_add "$TODAY" "-$days_back"
        ;;
      "last Friday"|"last friday")
        local target=5
        local days_back=$(( (DOW_NUM - target + 7) % 7 ))
        [[ $days_back -eq 0 ]] && days_back=7
        date_add "$TODAY" "-$days_back"
        ;;
      *"weeks ago")
        local n=$(echo "$expr" | grep -oE '[0-9]+')
        date_add "$TODAY" "-$((n * 7))"
        ;;
      *"months ago")
        local n=$(echo "$expr" | grep -oE '[0-9]+')
        local y=$(echo "$TODAY" | cut -d'-' -f1)
        local m=$(echo "$TODAY" | cut -d'-' -f2 | sed 's/^0//')
        local d=$(echo "$TODAY" | cut -d'-' -f3)
        m=$((m - n))
        while [[ $m -le 0 ]]; do
          m=$((m + 12))
          y=$((y - 1))
        done
        printf "%04d-%02d-%s" "$y" "$m" "$d"
        ;;
      *)
        echo "ERROR: Cannot parse '$expr' on BSD date. Use GNU date for complex expressions." >&2
        return 1
        ;;
    esac
  fi
}

# Get today's info
TODAY=$(date "+%Y-%m-%d")
TODAY_DOW=$(date "+%A")
DOW_NUM=$(date "+%u")  # 1=Monday, 7=Sunday

# ---- Subcommand functions ----

compute_week() {
  DAYS_SINCE_MON=$((DOW_NUM - 1))
  CURR_MON=$(date_add "$TODAY" "-$DAYS_SINCE_MON")
  CURR_FRI=$(date_add "$CURR_MON" "4")

  PREV_MON=$(date_add "$CURR_MON" "-7")
  PREV_FRI=$(date_add "$PREV_MON" "4")

  # Extract years for range formatting
  CURR_MON_YEAR=$(echo "$CURR_MON" | cut -d'-' -f1)
  CURR_FRI_YEAR=$(echo "$CURR_FRI" | cut -d'-' -f1)
  PREV_MON_YEAR=$(echo "$PREV_MON" | cut -d'-' -f1)
  PREV_FRI_YEAR=$(echo "$PREV_FRI" | cut -d'-' -f1)

  # Format short dates
  CURR_MON_FMT=$(format_short "$CURR_MON")
  CURR_FRI_FMT=$(format_short "$CURR_FRI")
  PREV_MON_FMT=$(format_short "$PREV_MON")
  PREV_FRI_FMT=$(format_short "$PREV_FRI")

  # Build range strings (handle year boundaries)
  if [ "$CURR_MON_YEAR" = "$CURR_FRI_YEAR" ]; then
    CURR_WEEK_RANGE="$CURR_MON_FMT - $CURR_FRI_FMT, $CURR_FRI_YEAR"
  else
    CURR_WEEK_RANGE="$CURR_MON_FMT, $CURR_MON_YEAR - $CURR_FRI_FMT, $CURR_FRI_YEAR"
  fi

  if [ "$PREV_MON_YEAR" = "$PREV_FRI_YEAR" ]; then
    PREV_WEEK_RANGE="$PREV_MON_FMT - $PREV_FRI_FMT, $PREV_FRI_YEAR"
  else
    PREV_WEEK_RANGE="$PREV_MON_FMT, $PREV_MON_YEAR - $PREV_FRI_FMT, $PREV_FRI_YEAR"
  fi

  echo "DATE_CONTEXT:"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
  echo "  CURR_WEEK_START: $CURR_MON (Monday)"
  echo "  CURR_WEEK_END: $CURR_FRI (Friday)"
  echo "  CURR_WEEK_RANGE: $CURR_WEEK_RANGE"
  echo "  PREV_WEEK_START: $PREV_MON (Monday)"
  echo "  PREV_WEEK_END: $PREV_FRI (Friday)"
  echo "  PREV_WEEK_RANGE: $PREV_WEEK_RANGE"
}

compute_month() {
  local year=$(echo "$TODAY" | cut -d'-' -f1)
  local month=$(echo "$TODAY" | cut -d'-' -f2 | sed 's/^0//')

  # Current month boundaries
  local curr_month_start
  curr_month_start=$(printf "%04d-%02d-01" "$year" "$month")

  # Next month first day, then subtract 1 day for end of current month
  local next_month=$((month + 1))
  local next_year=$year
  if [[ $next_month -gt 12 ]]; then
    next_month=1
    next_year=$((year + 1))
  fi
  local next_month_start
  next_month_start=$(printf "%04d-%02d-01" "$next_year" "$next_month")
  local curr_month_end
  curr_month_end=$(date_add "$next_month_start" "-1")

  # Previous month boundaries
  local prev_month=$((month - 1))
  local prev_year=$year
  if [[ $prev_month -lt 1 ]]; then
    prev_month=12
    prev_year=$((year - 1))
  fi
  local prev_month_start
  prev_month_start=$(printf "%04d-%02d-01" "$prev_year" "$prev_month")
  local prev_month_end
  prev_month_end=$(date_add "$curr_month_start" "-1")

  local curr_month_fmt
  curr_month_fmt=$(format_short "$curr_month_start")
  local curr_month_end_fmt
  curr_month_end_fmt=$(format_short "$curr_month_end")
  local prev_month_fmt
  prev_month_fmt=$(format_short "$prev_month_start")
  local prev_month_end_fmt
  prev_month_end_fmt=$(format_short "$prev_month_end")

  echo "MONTH_CONTEXT:"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
  echo "  CURR_MONTH_START: $curr_month_start"
  echo "  CURR_MONTH_END: $curr_month_end"
  echo "  CURR_MONTH_RANGE: $curr_month_fmt - $curr_month_end_fmt, $year"
  echo "  PREV_MONTH_START: $prev_month_start"
  echo "  PREV_MONTH_END: $prev_month_end"
  echo "  PREV_MONTH_RANGE: $prev_month_fmt, $prev_year - $prev_month_end_fmt, $year"
}

compute_quarter() {
  local year=$(echo "$TODAY" | cut -d'-' -f1)
  local month=$(echo "$TODAY" | cut -d'-' -f2 | sed 's/^0//')

  # Calendar quarter (Q1=Jan-Mar, Q2=Apr-Jun, Q3=Jul-Sep, Q4=Oct-Dec)
  local quarter=$(( (month - 1) / 3 + 1 ))
  local q_start_month=$(( (quarter - 1) * 3 + 1 ))
  local q_end_month=$(( quarter * 3 ))
  local q_start
  q_start=$(printf "%04d-%02d-01" "$year" "$q_start_month")

  # Quarter end: first day of next quarter minus 1
  local next_q_month=$(( q_end_month + 1 ))
  local next_q_year=$year
  if [[ $next_q_month -gt 12 ]]; then
    next_q_month=1
    next_q_year=$((year + 1))
  fi
  local next_q_start
  next_q_start=$(printf "%04d-%02d-01" "$next_q_year" "$next_q_month")
  local q_end
  q_end=$(date_add "$next_q_start" "-1")

  # Fiscal year (Oct-Sep): FY starts in October of previous calendar year
  local fy_year
  local fy_start fy_end
  if [[ $month -ge 10 ]]; then
    fy_year=$((year + 1))
    fy_start="$year-10-01"
    fy_end="$fy_year-09-30"
  else
    fy_year=$year
    fy_start="$((year - 1))-10-01"
    fy_end="$year-09-30"
  fi

  # Fiscal quarter
  local fy_month_offset
  if [[ $month -ge 10 ]]; then
    fy_month_offset=$((month - 10 + 1))
  else
    fy_month_offset=$((month + 3))
  fi
  local fq=$(( (fy_month_offset - 1) / 3 + 1 ))

  echo "QUARTER_CONTEXT:"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
  echo "  CALENDAR_QUARTER: Q$quarter $year"
  echo "  QUARTER_START: $q_start"
  echo "  QUARTER_END: $q_end"
  echo "  FISCAL_YEAR: FY$fy_year (Oct-Sep)"
  echo "  FISCAL_QUARTER: FQ$fq"
  echo "  FY_START: $fy_start"
  echo "  FY_END: $fy_end"
}

compute_relative() {
  local expr=$1
  local resolved
  resolved=$(resolve_relative "$expr")
  if [[ $? -ne 0 || -z "$resolved" ]]; then
    echo "RELATIVE_DATE:"
    echo "  ERROR: Could not resolve '$expr'"
    return 1
  fi

  local resolved_dow
  if [[ "$DATE_FLAVOR" == "gnu" ]]; then
    resolved_dow=$(date -d "$resolved" "+%A")
  else
    local epoch=$(date_to_epoch "$resolved")
    resolved_dow=$(date -r "$epoch" "+%A")
  fi

  echo "RELATIVE_DATE:"
  echo "  EXPRESSION: $expr"
  echo "  RESOLVED: $resolved ($resolved_dow)"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
}

compute_iso() {
  local iso_week iso_year day_of_year
  if [[ "$DATE_FLAVOR" == "gnu" ]]; then
    iso_week=$(date -d "$TODAY" "+%V")
    iso_year=$(date -d "$TODAY" "+%G")
    day_of_year=$(date -d "$TODAY" "+%j")
  else
    iso_week=$(date -j -f "%Y-%m-%d" "$TODAY" "+%V" 2>/dev/null)
    iso_year=$(date -j -f "%Y-%m-%d" "$TODAY" "+%G" 2>/dev/null || echo "$TODAY" | cut -d'-' -f1)
    day_of_year=$(date -j -f "%Y-%m-%d" "$TODAY" "+%j" 2>/dev/null)
  fi

  echo "ISO_CONTEXT:"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
  echo "  ISO_WEEK: W$iso_week"
  echo "  ISO_YEAR: $iso_year"
  echo "  ISO_WEEKDATE: $iso_year-W$iso_week-$DOW_NUM"
  echo "  DAY_OF_YEAR: $day_of_year"
}

compute_epoch() {
  local epoch
  if [[ "$DATE_FLAVOR" == "gnu" ]]; then
    epoch=$(date "+%s")
  else
    epoch=$(date "+%s")
  fi
  local millis="${epoch}000"

  echo "EPOCH_CONTEXT:"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
  echo "  UNIX_SECONDS: $epoch"
  echo "  UNIX_MILLIS: $millis"
}

compute_tz() {
  local tz_abbrev tz_offset
  tz_abbrev=$(date "+%Z")
  tz_offset=$(date "+%z")

  echo "TIMEZONE_CONTEXT:"
  echo "  TODAY: $TODAY ($TODAY_DOW)"
  echo "  TIMEZONE: $tz_abbrev"
  echo "  UTC_OFFSET: $tz_offset"
}

compute_range() {
  local date1=$1
  local date2=$2

  # Validate date formats
  if ! echo "$date1" | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
    echo "RANGE_ERROR: Invalid date format '$date1'. Use YYYY-MM-DD." >&2
    return 1
  fi
  if ! echo "$date2" | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
    echo "RANGE_ERROR: Invalid date format '$date2'. Use YYYY-MM-DD." >&2
    return 1
  fi

  local epoch1 epoch2
  epoch1=$(date_to_epoch "$date1")
  epoch2=$(date_to_epoch "$date2")

  if [[ -z "$epoch1" || -z "$epoch2" ]]; then
    echo "RANGE_ERROR: Could not parse dates." >&2
    return 1
  fi

  local diff_seconds=$((epoch2 - epoch1))
  local calendar_days=$((diff_seconds / 86400))

  # Count business days (Mon-Fri)
  local business_days=0
  local current="$date1"
  local step=1
  if [[ $calendar_days -lt 0 ]]; then
    step=-1
    calendar_days=$(( -calendar_days ))
  fi

  local i=0
  while [[ $i -lt $calendar_days ]]; do
    local dow
    if [[ "$DATE_FLAVOR" == "gnu" ]]; then
      dow=$(date -d "$current" "+%u")
    else
      local e=$(date_to_epoch "$current")
      dow=$(date -r "$e" "+%u")
    fi
    # Count if weekday (Mon=1 through Fri=5)
    if [[ $dow -ge 1 && $dow -le 5 ]]; then
      business_days=$((business_days + 1))
    fi
    current=$(date_add "$current" "$step")
    i=$((i + 1))
  done

  echo "RANGE_CONTEXT:"
  echo "  FROM: $date1"
  echo "  TO: $date2"
  echo "  CALENDAR_DAYS: $calendar_days"
  echo "  BUSINESS_DAYS: $business_days"
}

usage() {
  echo "Usage: chronos.sh [SUBCOMMAND] [ARGS]"
  echo ""
  echo "Subcommands:"
  echo "  (none), week     Week boundaries (DATE_CONTEXT block)"
  echo "  month            Current/previous month boundaries"
  echo "  quarter          Calendar quarter, fiscal year (Oct-Sep)"
  echo "  relative \"expr\"  Resolve relative date expression"
  echo "  iso              ISO week number, year, day-of-year"
  echo "  epoch            Unix timestamp (seconds and milliseconds)"
  echo "  tz               Timezone abbreviation and UTC offset"
  echo "  range D1 D2      Calendar and business days between dates"
  echo "  all              All of the above combined"
  echo ""
  echo "Examples:"
  echo "  chronos.sh                        # Week context (default)"
  echo "  chronos.sh month                  # Month boundaries"
  echo "  chronos.sh quarter                # Quarter and fiscal year"
  echo "  chronos.sh relative \"3 days ago\"  # Resolve expression"
  echo "  chronos.sh range 2026-01-01 2026-02-01"
  echo "  chronos.sh all                    # Everything"
}

# ---- Main dispatch ----

case "${1:-week}" in
  -h|--help)
    usage
    ;;
  week|"")
    compute_week
    ;;
  month)
    compute_month
    ;;
  quarter)
    compute_quarter
    ;;
  relative)
    if [[ -z "${2:-}" ]]; then
      echo "Usage: chronos.sh relative \"expression\"" >&2
      exit 1
    fi
    compute_relative "$2"
    ;;
  iso)
    compute_iso
    ;;
  epoch)
    compute_epoch
    ;;
  tz)
    compute_tz
    ;;
  range)
    if [[ -z "${2:-}" || -z "${3:-}" ]]; then
      echo "Usage: chronos.sh range YYYY-MM-DD YYYY-MM-DD" >&2
      exit 1
    fi
    compute_range "$2" "$3"
    ;;
  all)
    compute_week
    echo ""
    compute_month
    echo ""
    compute_quarter
    echo ""
    compute_iso
    echo ""
    compute_epoch
    echo ""
    compute_tz
    ;;
  *)
    echo "Unknown subcommand: $1" >&2
    usage >&2
    exit 1
    ;;
esac
