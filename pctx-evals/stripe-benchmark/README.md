# Stripe Benchmark Evals

This directory contains the Stripe benchmark evaluation suite with fixtures.

## Quick Start

After running evals, reset your Stripe test account to the clean fixture state:

```bash
export STRIPE_SECRET_KEY='sk_test_...'
./reset-fixtures.sh
```

Or pass the key directly:

```bash
./reset-fixtures.sh sk_test_...
```

## What It Does

The reset script:
- Archives duplicate/extra products (keeps them in account but inactive)
- Deletes duplicate/extra coupons
- Archives prices associated with inactive products
- Verifies final state matches `fixtures.json` exactly

## Expected State

After reset, your account will have (active resources only):
- **500 customers**
- **21 products**
- **21 prices**
- **20 coupons**

This matches the exact state defined in `fixtures.json` for consistent eval runs.

## Files

- `fixtures.json` - Stripe test data (500 customers, products, prices, coupons)
- `reset-fixtures.sh` - One-command reset to clean state
- `field.toml` - Eval configuration
- `01-get-balance/` through `12-create-payment-link/` - Individual eval test cases
