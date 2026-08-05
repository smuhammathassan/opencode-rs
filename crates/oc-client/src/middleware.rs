//! Protocol middleware.
//!
//! The reference Protocol declares two HTTP middleware services
//! (`reference/packages/protocol/src/middleware/authorization.ts` and
//! `middleware/schema-error.ts`). They are server-side: `Authorization` emits
//! `UnauthorizedError` (401) and `SchemaErrorMiddleware` emits
//! `InvalidRequestError` (400) on schema failures. On the client these surface
//! as the `[401, 400]` entries in every endpoint's `declaredStatuses` and the
//! `_tag`-discriminated error decoding in `crate::error`.
//!
//! The reference has no logging or retry middleware. Retries are offered here
//! as an opt-in client-side extension via [`crate::RetryPolicy`].

pub use crate::transport::RetryPolicy;
