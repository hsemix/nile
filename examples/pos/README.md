# Northstar POS interface

An Aronium-inspired retail counter built with Nile's current HTML/CSS/JS features.

```sh
cargo run -p nile-shell -- examples/pos/index.html
```

The opening cart contains a sample sale so the checkout layout can be reviewed
immediately. Clear starts an empty cart. New sale holds a nonempty cart first.

- Search by name or barcode and filter by category; click products to add them.
- Adjust quantities, remove items, choose a sample customer and apply a percentage discount.
- Hold a sale and resume it from Orders. The current sale is held if necessary.
- Checkout with cash and calculate change; insufficient cash is rejected.
- Card and mobile money are simulated choices for evaluating the flow.
- Complete a sample sale to view a receipt and update the in-memory stock.
- Products shows the catalog and stock; Reports shows completed sample sales and receipts.
- F3 focuses product search; F8 opens checkout; Escape closes dialogs.

Example barcode: `200000000001` adds whole milk when entered in search followed
by Enter. The catalog contains twelve products. Amounts use whole Ugandan
shillings; prices include tax and no additional tax is calculated.

All data lives in JavaScript memory and resets on reload. No database, payment
processing, printing or cashier authentication is connected. The sample is for
judging the interface before implementing a Rust/SQLite application backend.
Product artwork is drawn with CSS; no remote assets are needed.
