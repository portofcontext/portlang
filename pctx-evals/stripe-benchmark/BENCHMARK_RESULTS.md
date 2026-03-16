# Stripe API via CLI vs MCP vs MCP + Code Mode: A Benchmark

The internet is currently arguing about whether AI agents should use CLIs or MCP servers to interact with external services. CLI advocates point to simplicity and ubiquity — every service already has one. MCP advocates point to structured interfaces, type safety, and auditability. We ran a benchmark to add some data to the conversation.

We also tested a third approach: **MCP with Code Mode**, where the agent writes code to interact with Stripe rather than calling tools directly. The results were striking.

## The Benchmark

We used [pctx](https://github.com/portofcontext/pctx) — a portlang eval runner — to execute 12 Stripe tasks three times, once per access method:

1. **Stripe CLI** — a single shell tool, `stripe {args}`, backed by the Stripe CLI binary
2. **Stripe MCP** — the official `@stripe/mcp` server, giving the agent typed tool calls
3. **MCP + Code Mode** — same MCP server, but the agent writes Python/code to accomplish tasks

All three runs used `claude-sonnet-4.6` at temperature 0.0. Same model, same tasks, same test account.

**The 12 tasks** ranged from simple read operations to complex multi-step write operations:

| # | Task |
|---|------|
| 01 | Get account balance |
| 02 | List customers (limit 3) |
| 03 | Get balance again (control) |
| 04 | Find the most expensive product |
| 05 | Find a customer by employer |
| 06 | Find active subscription price |
| 07 | Count disputes |
| 08 | Find active subscription |
| 09 | Find customers with disputes |
| 10 | Create a coupon |
| 11 | Create an invoice |
| 12 | Create a payment link |

Every run achieved **12/12 (100%) pass rate**. The question isn't *whether* the agent can do it — it's *how much it costs*.

---

## Results at a Glance

| Method | Pass Rate | Total Cost | Avg Tokens/Task | vs Code Mode |
|--------|-----------|------------|-----------------|--------------|
| **MCP + Code Mode** | 12/12 | **$0.98** | **24,577** | baseline |
| Stripe MCP | 12/12 | $1.60 | 42,248 | +63% cost |
| Stripe CLI | 12/12 | $2.22 | 59,296 | +127% cost |

Code Mode with MCP was **2.3x cheaper** than CLI and **1.6x cheaper** than vanilla MCP, with dramatically lower token consumption across the board.

---

## Per-Task Breakdown

### Simple Retrieval Tasks (01–03, 06, 08)

For simple read operations, all three approaches converged in 2 steps. The differences here are about baseline overhead, not task complexity:

| Task | CLI Steps | CLI Tokens | MCP Steps | MCP Tokens | Code Mode Steps | Code Mode Tokens |
|------|-----------|------------|-----------|------------|-----------------|------------------|
| 01_get_balance | 2 | 3,001 | 2 | 19,172 | 2 | 12,027 |
| 02_list_customers | 2 | 4,599 | 2 | 19,267 | 2 | 12,212 |
| 03_get_balance | 2 | 3,104 | 2 | 19,276 | 2 | 12,060 |
| 06_subscription_price | 2 | 4,739 | 2 | 19,459 | 2 | 12,063 |
| 08_find_subscription | 2 | 4,657 | 2 | 19,397 | 2 | 12,005 |

The CLI approach has low token counts on simple tasks — the CLI command is terse and the response is compact. But MCP includes rich schema context in every request, which adds overhead even for trivial calls. Code Mode lands in the middle: it carries MCP schema overhead but uses structured outputs to stay focused.

### Complex Read Tasks (04, 05, 07, 09)

| Task | CLI Steps | CLI Tokens | MCP Steps | MCP Tokens | Code Mode Steps | Code Mode Tokens |
|------|-----------|------------|-----------|------------|-----------------|------------------|
| 04_expensive_product | 3 | 75,302 | 3 | 37,017 | 5 | 58,138 |
| 05_find_by_employer | 2 | 53,349 | 4 | 39,705 | 4 | 24,585 |
| 07_count_disputes | 2 | 5,859 | 4 | 39,808 | 3 | 18,773 |
| 09_customers_disputes | 4 | 19,543 | 4 | 40,873 | 6 | 40,195 |

The CLI starts showing bloat on complex tasks — task 04 consumed 75K tokens through CLI versus 37K via MCP. When the CLI response includes large JSON payloads inline in the context window, costs spike fast. MCP's structured tool responses are more context-window-friendly, and Code Mode's ability to loop over data programmatically keeps token counts down.

### Write/Creation Tasks (10, 11, 12)

This is where the differences become dramatic:

| Task | CLI Steps | CLI Tokens | MCP Steps | MCP Tokens | Code Mode Steps | Code Mode Tokens |
|------|-----------|------------|-----------|------------|-----------------|------------------|
| 10_create_coupon | 4 | 23,659 | 4 | 43,929 | 6 | 41,266 |
| **11_create_invoice** | **19** | **497,556** | **12** | **168,480** | **4** | **38,847** |
| 12_create_payment_link | 7 | 16,187 | 4 | 40,587 | 2 | 12,757 |

**Task 11 — Create an Invoice — is the story of this benchmark.**

- CLI: 19 steps, 497,556 tokens, $1.52
- MCP: 12 steps, 168,480 tokens, $0.53
- Code Mode: 4 steps, 38,847 tokens, $0.13

The CLI agent had to discover the invoice creation workflow through trial and error — running commands, reading output, adjusting arguments, running again. Creating an invoice requires several sub-steps (finding a customer, attaching line items, finalizing), and the CLI agent burned nearly half a million tokens navigating that sequence through raw shell output.

The MCP agent did better — structured tool schemas gave it a map of the API upfront. But it still took 12 steps because each MCP call is one discrete action.

The Code Mode agent wrote a short script that orchestrated the entire workflow, then executed it. Four steps, done.

---

## The CLI vs MCP Debate: What the Data Says

### The Case for MCP

**Standardization.** MCP gives the agent a typed, discoverable interface. The agent doesn't need to guess flag names, remember CLI argument formats, or parse freeform text output. This pays off especially on complex tasks — task 11 took 58% fewer steps with MCP than CLI.

**Auditability.** Every MCP call is a structured, logged event: tool name, input parameters, output. With CLI, you get a shell command string and stdout. If you want to audit what your agent did to your Stripe account — and you should want that — MCP's structured call log is significantly more useful.

**Reliability.** The CLI agent's 19-step spiral on task 11 isn't just expensive — it's fragile. More steps means more opportunities to fail, hallucinate an argument, or get stuck in a recovery loop. Structured interfaces reduce the surface area for failure.

### The Case Against Raw CLI for Agentic Use

Raw CLI was originally designed for humans. A human reading `stripe customers list --limit 3` output has context, can scan JSON, knows when something looks wrong. An agent piping that output back into its context window as raw text doesn't get the same signal efficiency. Every byte of CLI output is context window pressure.

The CLI performed well on the simplest tasks (where output is compact) but fell apart on complex ones. The 19-step invoice creation is not an anomaly — it's what happens when an agent needs to orchestrate multi-step operations through a text interface.

---

## Why Code Mode Changes the Equation

The most interesting finding isn't CLI vs MCP — it's Code Mode.

When Code Mode is enabled, the agent doesn't call tools one at a time. It writes code that uses those tools, then runs the code. This shifts the task structure from:

```
observe → think → call tool → observe → think → call tool → ...
```

to:

```
observe → think → write program → run program
```

For the invoice task, instead of issuing 12 sequential MCP calls, the Code Mode agent wrote something like:

```python
# find customer
customer = stripe.customers.list(email="...")[0]

# create invoice
invoice = stripe.invoices.create(customer=customer.id, ...)

# add line items
stripe.invoice_items.create(invoice=invoice.id, amount=..., ...)

# finalize
stripe.invoices.finalize_invoice(invoice.id)
```

Four steps. The LLM knows how to write that code extremely well — Stripe's API, Python patterns, control flow — because this is essentially what the entire internet's worth of training data looks like. CLI argument syntax and MCP tool-call JSON are far rarer in training data.

This is the under-discussed benefit of Code Mode: **the LLM's strongest training signal is code**. When you let it express business logic as code rather than as a sequence of tool calls, you're playing to its strengths.

---

## Full Cost Comparison

| Task | CLI Cost | MCP Cost | Code Mode Cost |
|------|----------|----------|----------------|
| 01_get_balance | $0.012 | $0.060 | $0.039 |
| 02_list_customers | $0.018 | $0.060 | $0.041 |
| 03_get_balance | $0.012 | $0.060 | $0.039 |
| 04_expensive_product | $0.231 | $0.116 | $0.192 |
| 05_find_by_employer | $0.163 | $0.126 | $0.080 |
| 06_subscription_price | $0.018 | $0.062 | $0.040 |
| 07_count_disputes | $0.022 | $0.126 | $0.064 |
| 08_find_subscription | $0.018 | $0.061 | $0.039 |
| 09_customers_disputes | $0.068 | $0.130 | $0.132 |
| 10_create_coupon | $0.077 | $0.137 | $0.137 |
| 11_create_invoice | **$1.520** | $0.531 | **$0.129** |
| 12_create_payment_link | $0.059 | $0.129 | $0.044 |
| **Total** | **$2.22** | **$1.60** | **$0.98** |

---

## Takeaways

**1. All three methods work.** 100% pass rates across 36 task executions. The question isn't capability, it's efficiency.

**2. MCP is better than CLI for agentic use.** Structured interfaces reduce steps, reduce token consumption on complex tasks, and produce auditable call logs. The 37% cost reduction over CLI is real.

**3. Code Mode is better still.** When the agent can write code to orchestrate multi-step operations, it leverages its strongest training signal. The 11x cost reduction on invoice creation (CLI → Code Mode) is not a fluke — it reflects a fundamentally more efficient execution strategy.

**4. Task complexity amplifies the differences.** On simple 2-step read operations, the method barely matters. On complex creation workflows, the choice of tool interface drives order-of-magnitude cost differences.

**5. Watch out for unbounded CLI runs.** The 497K token invoice creation via CLI isn't just expensive — it's unpredictable. If you're building production agents on top of CLI interfaces, one complex task can cost as much as 40 simple ones.

---

## How This Was Run

These benchmarks were run using [pctx](https://github.com/portofcontext/pctx), an open-source eval runner built on [portlang](https://github.com/portofcontext/portlang). The field configurations are in [pctx-evals/stripe-benchmark/](https://github.com/portofcontext/portlang/tree/main/pctx-evals/stripe-benchmark). All runs used `claude-sonnet-4.6` at temperature 0.0 against a live Stripe test account.

The HTML trajectory dashboards linked above show the full step-by-step execution for every task in every run, including tool calls, responses, and token counts.
