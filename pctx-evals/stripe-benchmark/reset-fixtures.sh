#!/bin/bash

# Reset Stripe account to match fixtures.json exactly
# Usage: ./reset-fixtures.sh <stripe_secret_key>
# Or:    export STRIPE_SECRET_KEY=sk_test_... && ./reset-fixtures.sh

set -e

STRIPE_KEY="${1:-$STRIPE_SECRET_KEY}"

if [ -z "$STRIPE_KEY" ]; then
    echo "Error: STRIPE_SECRET_KEY not provided"
    echo "Usage: $0 <stripe_secret_key>"
    echo "   or: export STRIPE_SECRET_KEY=sk_test_... && $0"
    exit 1
fi

echo "==================================================================="
echo "  RESETTING STRIPE ACCOUNT TO MATCH fixtures.json"
echo "==================================================================="
echo ""

# Create temp directory for data
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Load expected names from fixtures (without quotes)
echo "Loading expected resources from fixtures.json..."
jq -r '.fixtures[] | select(.path == "/v1/products") | .params.name' fixtures.json | sort > "$TMPDIR/expected_products.txt"
jq -r '.fixtures[] | select(.path == "/v1/coupons") | .params.name' fixtures.json | sort > "$TMPDIR/expected_coupons.txt"

# Fetch all resources from Stripe
echo "Fetching resources from Stripe..."

# Function to fetch all items with pagination
fetch_all() {
    local endpoint=$1
    local params="${2:-}"
    local all_items="[]"
    local starting_after=""
    local max_pages=20
    local page=0

    while [ $page -lt $max_pages ]; do
        page=$((page + 1))

        if [ -z "$starting_after" ]; then
            url="https://api.stripe.com/v1/${endpoint}?limit=100${params}"
        else
            url="https://api.stripe.com/v1/${endpoint}?limit=100${params}&starting_after=$starting_after"
        fi

        response=$(curl -s "$url" -H "Authorization: Bearer $STRIPE_KEY")
        data=$(echo "$response" | jq -r '.data')
        has_more=$(echo "$response" | jq -r '.has_more')

        all_items=$(echo "$all_items $data" | jq -s 'add')

        if [ "$has_more" != "true" ]; then
            break
        fi

        starting_after=$(echo "$data" | jq -r '.[-1].id')
    done

    echo "$all_items"
}

products=$(fetch_all "products")
coupons=$(fetch_all "coupons")
customers=$(fetch_all "customers")

echo "$products" > "$TMPDIR/products.json"
echo "$coupons" > "$TMPDIR/coupons.json"
echo "$customers" > "$TMPDIR/customers.json"

# Count current state
customer_count=$(echo "$customers" | jq 'length')
product_count=$(echo "$products" | jq 'length')
coupon_count=$(echo "$coupons" | jq 'length')

echo ""
echo "Current state:"
echo "  Customers: $customer_count"
echo "  Products: $product_count (all, including archived)"
echo "  Coupons: $coupon_count"
echo ""

# Archive extra/duplicate products
echo "==================================================================="
echo "  STEP 1: Archiving extra/duplicate products"
echo "==================================================================="
echo ""

archived_count=0
echo "" > "$TMPDIR/seen_products.txt"

while IFS= read -r line; do
    id=$(echo "$line" | jq -r '.id')
    name=$(echo "$line" | jq -r '.name')
    active=$(echo "$line" | jq -r '.active')

    # Skip if already archived
    if [ "$active" != "true" ]; then
        continue
    fi

    should_archive=false
    reason=""

    # Check if in expected list
    if ! grep -Fxq "$name" "$TMPDIR/expected_products.txt"; then
        should_archive=true
        reason="NOT in fixtures"
    # Check if duplicate
    elif grep -Fxq "$name" "$TMPDIR/seen_products.txt"; then
        should_archive=true
        reason="DUPLICATE"
    else
        echo "$name" >> "$TMPDIR/seen_products.txt"
    fi

    if [ "$should_archive" = true ]; then
        echo "  Archiving: $name ($reason)"
        curl -s -X POST "https://api.stripe.com/v1/products/$id" \
            -H "Authorization: Bearer $STRIPE_KEY" \
            -d "active=false" > /dev/null
        archived_count=$((archived_count + 1))
    fi
done < <(jq -c '.[]' "$TMPDIR/products.json")

echo ""
echo "Archived $archived_count products"

# Delete extra/duplicate coupons
echo ""
echo "==================================================================="
echo "  STEP 2: Deleting extra/duplicate coupons"
echo "==================================================================="
echo ""

deleted_count=0
echo "" > "$TMPDIR/seen_coupons.txt"

while IFS= read -r line; do
    id=$(echo "$line" | jq -r '.id')
    name=$(echo "$line" | jq -r '.name')

    should_delete=false
    reason=""

    # Check if in expected list
    if ! grep -Fxq "$name" "$TMPDIR/expected_coupons.txt"; then
        should_delete=true
        reason="NOT in fixtures"
    # Check if duplicate
    elif grep -Fxq "$name" "$TMPDIR/seen_coupons.txt"; then
        should_delete=true
        reason="DUPLICATE"
    else
        echo "$name" >> "$TMPDIR/seen_coupons.txt"
    fi

    if [ "$should_delete" = true ]; then
        echo "  Deleting: $name ($reason)"
        curl -s -X DELETE "https://api.stripe.com/v1/coupons/$id" \
            -H "Authorization: Bearer $STRIPE_KEY" > /dev/null
        deleted_count=$((deleted_count + 1))
    fi
done < <(jq -c '.[]' "$TMPDIR/coupons.json")

echo ""
echo "Deleted $deleted_count coupons"

# Re-fetch coupons after deletions for accurate final verification
coupons=$(fetch_all "coupons")

# Delete extra customers (if any)
echo ""
echo "==================================================================="
echo "  STEP 3: Checking customers"
echo "==================================================================="
echo ""

expected_customer_count=$(jq '[.fixtures[] | select(.path == "/v1/customers")] | length' fixtures.json)

if [ "$customer_count" -gt "$expected_customer_count" ]; then
    echo "WARNING: Found $customer_count customers, expected $expected_customer_count"
    echo "Extra customers detected. You may want to manually review."
    echo "This script does not auto-delete customers to avoid data loss."
elif [ "$customer_count" -eq "$expected_customer_count" ]; then
    echo "✓ Customer count matches ($customer_count)"
else
    echo "WARNING: Found $customer_count customers, expected $expected_customer_count"
    echo "Missing customers! You may need to reload fixtures."
fi

# Archive excess prices per active product (e.g. agent-created duplicate prices)
echo ""
echo "==================================================================="
echo "  STEP 4: Archiving excess prices per active product"
echo "==================================================================="
echo ""

active_products_step4=$(fetch_all "products" "&active=true")
active_prices_step4=$(fetch_all "prices" "&active=true")

excess_price_count=0
while IFS= read -r product_line; do
    product_id=$(echo "$product_line" | jq -r '.id')
    product_name=$(echo "$product_line" | jq -r '.name')

    # Expected price count per product name (from fixtures.json)
    expected=1
    if [[ "$product_name" == "Pro Plan" ]] || [[ "$product_name" == "Starter Plan" ]] || [[ "$product_name" == "Business Plan" ]]; then
        expected=2
    elif [[ "$product_name" == "99.99% Uptime SLA" ]] || [[ "$product_name" == "Audit Log & Data Export" ]] || [[ "$product_name" == "Sandbox Environment" ]]; then
        expected=0
    fi

    product_prices=$(echo "$active_prices_step4" | jq -c "[.[] | select(.product == \"$product_id\")] | sort_by(.created)")
    price_count=$(echo "$product_prices" | jq 'length')

    if [ "$price_count" -gt "$expected" ]; then
        to_archive=$((price_count - expected))
        echo "  Product '$product_name' has $price_count prices (expected $expected) — archiving $to_archive newest"
        echo "$product_prices" | jq -c ".[-${to_archive}:][]" | while IFS= read -r price_line; do
            price_id=$(echo "$price_line" | jq -r '.id')
            echo "    Archiving $price_id"
            curl -s -X POST "https://api.stripe.com/v1/prices/$price_id" \
                -H "Authorization: Bearer $STRIPE_KEY" \
                -d "active=false" > /dev/null
        done
        excess_price_count=$((excess_price_count + to_archive))
    fi
done < <(echo "$active_products_step4" | jq -c '.[]')

echo ""
echo "Archived $excess_price_count excess prices"

# Verify final state
echo ""
echo "==================================================================="
echo "  FINAL VERIFICATION (Active resources only)"
echo "==================================================================="
echo ""

active_products=$(fetch_all "products" "&active=true")
active_prices=$(fetch_all "prices" "&active=true")

final_product_count=$(echo "$active_products" | jq 'length')
final_price_count=$(echo "$active_prices" | jq 'length')
final_coupon_count=$(echo "$coupons" | jq '[.[] | select(.valid == true)] | length')

expected_products=21
expected_prices=21
expected_coupons=20

echo "Expected:"
echo "  Customers: $expected_customer_count"
echo "  Products: $expected_products"
echo "  Prices: $expected_prices"
echo "  Coupons: $expected_coupons"
echo ""

echo "Actual (active only):"
echo "  Customers: $customer_count"
echo "  Products: $final_product_count"
echo "  Prices: $final_price_count"
echo "  Coupons: $final_coupon_count"
echo ""

all_match=true

if [ "$customer_count" -eq "$expected_customer_count" ]; then
    echo "✓ Customers: MATCH"
else
    echo "✗ Customers: MISMATCH"
    all_match=false
fi

if [ "$final_product_count" -eq "$expected_products" ]; then
    echo "✓ Products: MATCH"
else
    echo "✗ Products: MISMATCH (diff: $((final_product_count - expected_products)))"
    all_match=false
fi

if [ "$final_price_count" -eq "$expected_prices" ]; then
    echo "✓ Prices: MATCH"
else
    echo "✗ Prices: MISMATCH (diff: $((final_price_count - expected_prices)))"
    all_match=false

    # If there are extra prices, archive them
    if [ "$final_price_count" -gt "$expected_prices" ]; then
        echo ""
        echo "  Archiving extra prices..."

        # Get product IDs that should be active
        active_product_ids=$(echo "$active_products" | jq -r '.[].id')

        # Archive prices for inactive products
        while IFS= read -r price_line; do
            price_id=$(echo "$price_line" | jq -r '.id')
            product_id=$(echo "$price_line" | jq -r '.product')

            # Check if product is in active list
            if ! echo "$active_product_ids" | grep -q "$product_id"; then
                echo "    Archiving price $price_id (product $product_id is inactive)"
                curl -s -X POST "https://api.stripe.com/v1/prices/$price_id" \
                    -H "Authorization: Bearer $STRIPE_KEY" \
                    -d "active=false" > /dev/null
            fi
        done < <(echo "$active_prices" | jq -c '.[]')
    fi
fi

if [ "$final_coupon_count" -eq "$expected_coupons" ]; then
    echo "✓ Coupons: MATCH"
else
    echo "✗ Coupons: MISMATCH (diff: $((final_coupon_count - expected_coupons)))"
    all_match=false
fi

echo ""
echo "==================================================================="
if [ "$all_match" = true ]; then
    echo "  ✓ RESET COMPLETE - Account matches fixtures.json"
else
    echo "  ⚠ RESET INCOMPLETE - Some mismatches remain"
    echo "    Run script again or check manually"
fi
echo "==================================================================="
