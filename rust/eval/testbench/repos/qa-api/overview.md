# PaymentGateway

The PaymentGateway is the component that talks to the upstream card processor.

## Retry policy

The PaymentGateway retries failed charges using exponential backoff: it starts
with a base delay of 200ms and doubles the delay after every failed attempt
(200ms, 400ms, 800ms, ...). It gives up after a maximum of 5 attempts, at which
point the charge is marked permanently failed and the error is surfaced to the
caller. Only transient upstream errors (HTTP 5xx, timeouts) are retried; a hard
decline is never retried.
