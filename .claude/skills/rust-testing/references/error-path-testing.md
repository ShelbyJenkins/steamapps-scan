# Error-path testing

For each meaningful rejection, prove that the input or state is rejected, the correct error category is returned, actionable context names the failing path or field, and no partial mutation or persistence remains unless explicitly documented.

Test recovery and retry behavior when supported. Include malformed, missing, contradictory, stale, out-of-range, permission, and interruption cases according to the actual boundary. A useful review question is: if this error return were removed or changed to continue, which test would fail?

Distinguish malformed input, unsupported behavior, ownership conflicts, unavailable resources, and internal failures when callers depend on that distinction. Do not assert only that some error occurred.

For multi-phase writes, verify that validation completes before mutation, ownership metadata remains trustworthy after failure, and a later retry can recover. Test that stale cleanup cannot remove neighboring unmanaged content.

When a real defect supplies a failing input, minimize it and keep it as a regression fixture. Record any environmental failure that could not be reproduced rather than manufacturing confidence.

