---
title: Function reference
nav_order: 6
---

# Function reference
{: .no_toc }

Cellar's functions follow Excel: the same names, arguments and results.
While you type a formula, Cellar shows each function's arguments.

1. TOC
{:toc}

## Maths and statistics

ABS, ACOS, ASIN, ATAN, ATAN2, AVERAGE, CEILING, COMBIN, CORREL, COS, COSH,
DEGREES, EVEN, EXP, FACT, FLOOR, FREQUENCY, GCD, INT, INTERCEPT, LARGE,
LCM, LN, LOG, MAX, MEDIAN, MIN, MOD, MROUND, ODD, PERCENTILE.INC, PI,
POWER, RADIANS, RAND, RANDBETWEEN, RANK.EQ, ROUND, ROUNDDOWN, ROUNDUP, RSQ,
SIGN, SIN, SINH, SLOPE, SMALL, SQRT, STDEV, STDEV.P, STDEV.S, SUM,
SUMPRODUCT, TAN, TANH, TRUNC, VAR.P, VAR.S

| Example | Result |
|---|---|
| `=SUM(B2:B10)` | The total of the range |
| `=ROUND(3.14159, 2)` | 3.14 |
| `=LARGE(B2:B10, 2)` | The second largest value |
| `=SUMPRODUCT(B2:B10, C2:C10)` | Each price times its quantity, added up |

## Logic

AND, FALSE, IF, IFERROR, IFNA, IFS, NOT, OR, SWITCH, TRUE, XOR

| Example | Result |
|---|---|
| `=IF(B2>100, "high", "low")` | One of two values |
| `=IFS(B2>=90, "A", B2>=80, "B", TRUE, "C")` | The first match |
| `=IFERROR(A2/B2, 0)` | 0 instead of an error |

## Lookup and reference

AVERAGEIF, COUNTIF, GETPIVOTDATA, HLOOKUP, INDEX, INDIRECT, MATCH, OFFSET,
SUMIF, VLOOKUP, XLOOKUP

| Example | Result |
|---|---|
| `=XLOOKUP("West", A2:A9, C2:C9)` | The value in C beside "West" in A |
| `=VLOOKUP(E2, A2:C9, 3, FALSE)` | The third column of the row that matches E2 |
| `=INDEX(A2:C9, MATCH("West", A2:A9, 0), 2)` | The same, with INDEX and MATCH |
| `=SUMIF(A2:A9, "West", C2:C9)` | The sum of C where A is "West" |
| `=COUNTIF(C2:C9, ">100")` | How many values exceed 100 |

Criteria in `SUMIF`, `COUNTIF` and `AVERAGEIF` ignore case and accept
`>`, `>=`, `<`, `<=`, `<>` and the wildcards `*` and `?`.

## Text

ARRAYTOTEXT, CHAR, CLEAN, CODE, CONCAT, DOLLAR, EXACT, FIND, FIXED, LEFT,
LEN, LOWER, MID, NUMBERVALUE, PROPER, REGEXEXTRACT, REGEXMATCH,
REGEXREPLACE, REPLACE, REPT, RIGHT, SEARCH, SUBSTITUTE, TEXT, TEXTAFTER,
TEXTBEFORE, TEXTJOIN, TRIM, UNICHAR, UNICODE, UPPER, VALUE

| Example | Result |
|---|---|
| `=A2 & " " & B2` | Two texts joined with a space |
| `=TEXTJOIN(", ", TRUE, A2:A5)` | The cells joined with commas |
| `=LEFT(A2, 3)` | The first three characters |
| `=TEXTBEFORE(A2, "@")` | The part of an e-mail address before the @ |
| `=REGEXEXTRACT(A2, "[0-9]+")` | The first number in a text |

## Date and time

DATE, DATEDIF, DATEVALUE, DAY, DAYS, EDATE, EOMONTH, HOUR, MINUTE, MONTH,
NETWORKDAYS, NOW, SECOND, TIME, TIMEVALUE, TODAY, WEEKDAY, WORKDAY, YEAR,
YEARFRAC

| Example | Result |
|---|---|
| `=TODAY()` | Today's date |
| `=DATEDIF(A2, TODAY(), "Y")` | Full years since the date in A2 |
| `=EOMONTH(A2, 0)` | The last day of that month |
| `=NETWORKDAYS(A2, B2)` | Working days between two dates |

## Finance

FV, NPV, PMT, PV

| Example | Result |
|---|---|
| `=PMT(0.05/12, 360, 200000)` | The monthly payment of a loan |
| `=NPV(0.08, B2:B6)` | Net present value of cash flows |

## Information

COUNT, COUNTA, ERROR.TYPE, ISBLANK, ISERR, ISERROR, ISNA, ISNUMBER, ISTEXT,
NA, TYPE

## Dynamic arrays and lambdas

BYCOL, BYROW, FILTER, LAMBDA, LET, MAKEARRAY, MAP, REDUCE, SCAN, SEQUENCE,
SORT, TRANSPOSE, UNIQUE

| Example | Result |
|---|---|
| `=SORT(A2:A9)` | The range sorted, spilled into the cells below |
| `=FILTER(A2:C50, C2:C50>100)` | Only the rows over 100 |
| `=LET(x, SUM(B2:B9), x / 8)` | A named intermediate result |
| `=MAP(A2:A5, LAMBDA(x, x * 2))` | Each value doubled |

## Special

| Function | What it does |
|---|---|
| `HYPERLINK(link, [text])` | A link; Ctrl+click opens it |
| `SPARKLINE(data, [type])` | A small chart in the cell: `"line"` (default) or `"column"` |
| `GETPIVOTDATA(data_field, pivot_table, [field, item], …)` | A value from a PivotTable |
| `GET(url)` | The text a web address returns |

`GET` needs network access, which is switched off, and the window has no
setting to switch it on yet.
{: .warning }
