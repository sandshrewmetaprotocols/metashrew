# BRC-20s

The following information is taken from the [Layer 1 Foundation](https://l1f.xyz/) which is a great resource for learning, discussing, and growing the BRC-20 standard.

## Overview

BRC-20 is a groundbreaking protocol that leverages the principles of [ordinal theory](https://docs.ordinals.com/overview.html) and [inscriptions](inscriptions.md) to bring fungibility to the Bitcoin blockchain. Conceived in March 2023 by @domodata, you can find the original documentation and white paper for BRC-20 [here](https://domo-2.gitbook.io/brc-20-experiment/).

At its core, BRC-20 simplifies the creation, minting, and transaction of fungible assets on the Bitcoin network. By operating at the foundational Layer 1 of Bitcoin, BRC-20s inherit the battle-tested security mechanisms of the blockchain, specifically preventing double spends by anchoring transferable balances to a single satoshi.

However, like all meta-protocols, BRC-20 requires efficient indexing for seamless functionality. Oyl's Sandshrew Indexer, built in collaboration with the Layer 1 Foundation, is one of the top indexers in the space. Excitingly, Oyl is actively working on open-sourcing the Sandshrew indexer, ensuring robust and accessible indexing support.

To explore BRC-20 comprehensively and access essential resources, visit the [Layer 1 Foundation's discourse page](https://l1f.discourse.group/latest). Here, you'll find an array of valuable assets, including comprehensive documentation, rule clarifications, setup guides, and insights into infrastructure and tooling. It's your one-stop hub for everything related to BRC-20 and its thriving ecosystem.

With BRC-20, the creation, minting, and transaction of fungible assets on the Bitcoin blockchain reaches new heights. The technical intricacies are detailed in the original documentation, providing an in-depth understanding of this innovative protocol. And with its steadfast grounding in Layer 1 Bitcoin, BRC-20 inherits the blockchain's time-tested security features, most notably guarding against double spends by locking balances to individual satoshis.

### **BRC-20: Unleashing Creative Possibilities**

BRC-20 is an open protocol that serves as a boundless canvas for innovation, offering a foundation for a multitude of creative applications. Consider the following examples:

**Asset Issuance:** BRC-20s act as versatile digital representations of assets, rights, or any fungible entities imaginable. This versatility spans from stablecoins and utility tokens to whimsical meme-based tokens, providing a platform for diverse financial instruments and digital collectibles.

**dApp Integration:** Developers can seamlessly integrate BRC-20s into decentralized applications (dApps) that harness the robust Bitcoin network. These applications cover a wide spectrum, ranging from yield generation and collateralized loans to staking mechanisms, ushering in a new era of decentralized finance (DeFi) and utility-driven dApps.

**Tokenization:** The BRC-20 standard simplifies the tokenization of virtually any asset or right, unlocking a treasure trove of possibilities. This includes token-gated communities, enabling exclusive access to digital spaces, and innovative DAO (Decentralized Autonomous Organization) voting mechanisms, fostering collaborative governance.

**Exchange Mechanism:** BRC-20s seamlessly integrate into the layer 1 Bitcoin network, making them readily exchangeable and tradable across various platforms. While currently accessible through order books, the integration into liquidity pool swaps is on the near horizon, promising enhanced liquidity and accessibility for traders and investors.

With BRC-20, the possibilities are limited only by your imagination. It's a versatile protocol that empowers developers and creators to explore new horizons in the world of decentralized finance, digital assets, and blockchain-based applications.

### **Why BRC-20?**

While various Bitcoin inscription protocols are emerging, BRC-20 distinguishes itself with several noteworthy features that make it a preferred choice over other standards:

* **Widest Adoption:** BRC-20 proudly boasts the most extensive adoption among fungible standards on the Bitcoin network, making it a trusted and well-established choice.
* **Simplified Interaction:** Unlike other protocols requiring dedicated wallet features, BRC-20 allows seamless interaction with just a Bitcoin wallet, thanks to its inscription tools.
* **Immutable Inscriptions:** BRC-20 takes a unique approach with immutable inscriptions, setting it apart from mutable smart contracts on alternative platforms, ensuring data integrity and security.
* **Cost-Effective Account Model:** Its account-based model minimizes complexity and offers cost-effectiveness compared to UTXO-based fungible token standards, providing a fair minting distribution.
* **User-Friendly Simplicity:** BRC-20's simplicity makes it the easiest fungible standard to understand, access, and utilize on the Bitcoin network, reducing barriers for developers and users.
* **Legibility:** No encoding or hashing systems are utilized, ensuring straightforward and transparent data handling.
* **Fair Distribution:** BRC-20 follows an open and free minting approach without prior announcements or pre-mines, guaranteeing provable fairness—an achievement that may prove challenging for future standards to replicate.
* **Exclusive Tickers:** Implementation of exclusive tickers minimizes the risk of scams and misinterpretations, enhancing user confidence.
* **Blockchain Agnosticism:** BRC-20's independence from UTXOs allows seamless integration with systems beyond the Bitcoin blockchain, increasing its versatility.
* **Decentralized Validation:** Instead of relying on a singular indexing truth, BRC-20 leverages a diverse pool of indexers to validate a consistent truth or state, reinforcing its decentralized nature and reliability.

## Working with BRC-20

In the world of BRC-20 tokens, the possibilities are endless, offering a canvas for innovation and creativity. As you embark on your journey to deploy, mint, and transfer these tokens, it's essential to navigate this space with confidence and understanding. In this section, we provide insights and guidance on the key processes involved in managing BRC-20 tokens.

**Deploying Your Unique Token**

The journey begins with the deployment of your custom BRC-20 token. This process empowers you to create a token tailored to your specific needs and use cases. Discover how to inscribe the deploy function to your ordinals-compatible wallet, configure desired parameters, and claim your unique ticker. Additionally, gain insights into the ownership of tickers and the "first is first" approach to public BRC-20 mints.

**Minting Tokens with Precision**

Once your BRC-20 is deployed, you can proceed to mint them. Learn how to initiate minting operations in batches while adhering to predefined limits. Understand the significance of valid transfer functions and explore examples that illustrate the nuances of minting, including what happens when minting approaches or exceeds the maximum supply.

**Efficient Token Transfers**

The transfer of BRC-20 tokens is a fundamental operation that requires careful consideration. Discover how to inscribe the transfer function to your ordinals-compatible wallet, ensuring the information provided is valid before proceeding. Learn about the criteria for validity, available balance, and the mechanics of balance changes. Dive into the importance of one-time use for each transfer inscription and avoid unintended consequences.

**Guidance and Helpful Notes**

Throughout these processes, it's crucial to keep in mind helpful notes that offer valuable insights and considerations. From ordinals compatibility to ticker ownership and the prioritization of events in the same block, these notes provide essential guidance to make your BRC-20 operations seamless and secure.

### Deploying a BRC-20

To create your custom BRC-20, you'll need to `deploy` the BRC-20 contract to your ordinals-compatible wallet while specifying the desired BRC-20 parameters. Here's how you can do it:

```markup
{ 
  "p": "brc-20",
  "op": "deploy",
  "tick": "ordi",
  "max": "21000000",
  "lim": "1000"
}
```

<table><thead><tr><th width="97">Key</th><th width="108.33333333333331">Required?</th><th>Description</th></tr></thead><tbody><tr><td>p</td><td>Yes</td><td>Protocol: Helps other systems identify and process brc-20 events</td></tr><tr><td>op</td><td>Yes</td><td>Operation: Type of event (Deploy, Mint, Transfer)</td></tr><tr><td>tick</td><td>Yes</td><td>Ticker: 4 letter identifier of the brc-20</td></tr><tr><td>max</td><td>Yes</td><td>Max supply: set max supply of the brc-20</td></tr><tr><td>lim</td><td>No</td><td>Mint limit: If letting users mint to themselves, limit per ordinals</td></tr><tr><td>dec</td><td>No</td><td>Decimals: set decimal precision, default to 18</td></tr></tbody></table>

### Minting BRC-20

After deploying your BRC-20, you can proceed to mint new tokens in batches, following the limitations defined in the token's `deploy` inscription. To mint BRC-20, inscribe the `mint` function to your ordinals-compatible wallet while adhering to the following guidelines:

* Ensure that the ticker you specify matches a BRC-20 that has not yet reached its fully diluted supply.
* If the BRC-20 has a designated mint limit, make certain not to exceed this limit.

Here's an example inscription to mint a BRC-20:

```
{ 
  "p": "brc-20",
  "op": "mint",
  "tick": "ordi",
  "amt": "1000"
}
```

<table><thead><tr><th width="85">Key</th><th width="126">Required?</th><th>Description</th></tr></thead><tbody><tr><td>p</td><td>Yes</td><td>Protocol: Helps other systems identify and process brc-20 events</td></tr><tr><td>op</td><td>Yes</td><td>Operation: Type of event (Deploy, Mint, Transfer)</td></tr><tr><td>tick</td><td>Yes</td><td>Ticker: 4 letter identifier of the brc-20</td></tr><tr><td>amt</td><td>Yes</td><td>Amount to mint: States the amount of the brc-20 to mint. Has to be less than "lim" above if stated</td></tr></tbody></table>

### Transferring BRC-20

```
{ 
  "p": "brc-20",
  "op": "transfer",
  "tick": "ordi",
  "amt": "100"
}
```

<table><thead><tr><th width="99">Key</th><th width="127">Required?</th><th>Description</th></tr></thead><tbody><tr><td>p</td><td>Yes</td><td>Protocol: Helps other systems identify and process brc-20 events</td></tr><tr><td>op</td><td>Yes</td><td>Operation: Type of event (Deploy, Mint, Transfer)</td></tr><tr><td>tick</td><td>Yes</td><td>Ticker: 4 letter identifier of the brc-20</td></tr><tr><td>amt</td><td>Yes</td><td>Amount to transfer: States the amount of the brc-20 to transfer.</td></tr></tbody></table>

### **Notes for BRC-20 Operations**

Here are some important considerations and notes to keep in mind when engaging in BRC-20 operations:

1. **Ordinals Compatibility:** Ensure that you only send inscriptions to ordinals-compatible wallet taproot addresses. Sending inscriptions to non-ordinals compatible addresses may result in unintended consequences.
2. **Mint Function Clarification:** Be aware that the transfer of a mint function does not result in a change in the token balance under any circumstances.
3. **One-Time Use:** Each transfer inscription can only be utilized once, so exercise caution to prevent unintended duplicate transactions.
4. **Ticker Ownership:** The first deployment of a ticker is the sole claimant to that ticker. Tickers are not case-sensitive (e.g., "DOGE" and "doge" are considered identical).
5. **Prioritization in the Same Block:** If multiple events occur within the same block, prioritization is determined by the order in which they were confirmed in that block, from the first to the last.
6. **Public BRC-20 Mints:** For public BRC-20 mints, the "first is first" approach is adopted, meaning the first "x" inscriptions that meet the deploy function parameters are granted a token balance.
7. **Balance Changes:** Changes in token balances occur only during the mint function and the second step of the transfer function.
8. **Exceeding Maximum Supply:** In the event that the first mint exceeds the maximum supply, the fraction that is valid will be applied to the balance. For example, if the maximum supply is 21,000,000, the circulating supply is 20,999,242, and a 1000 mint inscription is processed, a balance state of 758 will be applied.
9. **Inscription Process:** No valid actions can be initiated via the spending of an ordinals through transaction fees. If such a situation arises during the inscription process, the resulting inscription will be ignored. If it occurs during the second phase of the transfer process, the balance will be returned to the sender's available balance.
10. **Decimal Limitation:** The number of decimals for BRC-20 tokens cannot exceed 18 (default), ensuring consistency and predictability in token calculations.
11. **Standard Limitations:** BRC-20 standards are limited to uint128, and the maximum supply of tokens cannot exceed uint64\_max, adhering to defined technical constraints.

## Indexing BRC-20

In addition to understanding the usage rules, it's crucial to grasp the intricacies of indexing when dealing with BRC-20 inscriptions. This section delves into some of the indexing rules.

### Guidelines <a href="#rules" id="rules"></a>

The indexing guidelines primarily cater to users who wish to independently index BRC-20 state data. However, these directives also offer valuable troubleshooting insights for those encountering challenges.

* Inscriptions must have a MIME Type of "text/plain" or "application/json". To check this, split the mime type from ";" and check the first part without strip/trim etc.
* Inscription must be a valid JSON (not JSON5). Trailing commas invalidate the function.
* Leading or trailing spaces/tabs/newlines are allowed (and stripped/trimmed).
* JSON must have "p", "op", "tick" fields where "p"="brc-20", "op" in \["deploy", "mint", "transfer"]
* If op is deploy, JSON must have a "max" field. "lim" and "dec" fields are optional. If "dec" is not set, it will be counted as 18. If "lim" is not set it will be equal to "max".
* If op is mint or transfer, JSON must have an "amt" field.
* All op and field names must be in lower case.
* ALL NECESSARY JSON FIELDS MUST BE STRINGS. Numbers at max, lim, amt, dec etc. are not accepted. Extra fields which haven't been discussed here can be of any type.
* Numeric fields are not stripped/trimmed. "dec" field must have only digits, other numeric fields may have a single dot(".") for decimal representation (+,- etc. are not accepted). Decimal fields cannot start or end with dot (e.g. ".99" and "99." are invalid).
* Empty string for numeric field is invalid.
* 0 for numeric fields is invalid except for the "dec" field.
* If any decimal representation have more decimal digits than "dec" of ticker, the inscription will be counted as invalid (even if the extra digits are 0)
* The Maximum value of "dec" is 18.
* Max value of any numeric field is uint64\_max.
* "tick'' must be 4 bytes wide (UTF-8 is accepted). "tick '' is case insensitive, we use lowercase letters to track tickers (convert tick to lowercase before processing).
* If a deploy, mint or transfer is sent as fee to miner while inscribing, it must be ignored
* If a transfer is sent as fee in its first transfer, its amount must be returned to the sender immediately (instead of after all events in the block).
* If a mint has been deployed with more amt than lim, it will be ignored.
* If a transfer has been deployed with more amt than the available balance of that wallet, it will be ignored.
* All balances are followed using scriptPubKey since some wallets may not have an address attached to bitcoin.
* First a deploy inscription is inscribed. This will set the rules for this BRC-20 ticker. If the same ticker (case insensitive) has already been deployed, the second deployment will be invalid.
* Then anyone can inscribe mint inscriptions with the limits set in deploy inscription until the minted balance reaches to "max" set in deploy inscription.
* When a wallet mints a BRC-20 (inscribes a mint inscription to its address), its overall balance and available balance will increase.
* Wallets can inscribe transfer inscriptions with an amount up to their available balance.
* If a user inscribes a transfer inscription but does not transfer it, its overall balance will stay the same but its available balance will decrease.
* When this transfer inscription is transferred (not sent as fee to miner) the original wallet's overall balance will decrease but its available balance will stay the same. The receiver's overall balance and available balance will increase.
* If you transfer the transfer inscription to your own wallet, your available balance will increase and the transfer inscription will become used.
* A transfer inscription will become invalid/used after its first transfer.
* Buying, transferring mint and deploy inscriptions will not change anyone's balance.
* “fee” and “to” keys were for demo indexing purposes only. Inclusions have no effect on the function nor do they invalidate it.
* Cursed inscriptions including BRC-20 data are not recognized as valid. BRC-20 employs the ord client version 0.9 definition of an inscription.
* Balances sent to unspendable outputs are not returned to sender like with the fee instance. They can practically be considered burnt (notwithstanding a bitcoin update that enables transactions to be created with these keys in the future)

### Points to consider <a href="#points-to-consider" id="points-to-consider"></a>

* JS integer is actually a double floating point number and it can only hold 53-bit integers with full-precision. If you want to make an indexer with JS, use BigInt.
* Sent as fee rule is very important and needs to be handled carefully.
* Do not use string length for ticker length comparison. Some UTF-8 characters like most of the emojis use 4 bytes but their string length is 1 on most programming languages.
* Numeric fields do not accept numeric values. Use strings in JSON.
* Do not trim/strip any field.
* Do not use int/float conversion directly on numeric fields. For example some languages may accept "123asd" as valid integer with 123 value but this inscription is invalid.
* Check decimal length of amt, max, lim from string value. Extra 0's at the end of the decimal part may invalidate the token if there are more decimal digits than "dec" of the ticker.
* Beware of the reorgs. Since the order of transactions is very important in this protocol, a reorg will probably change the balances.

## **Resources and Further Reading:**

* [Original Documentation 53](https://domo-2.gitbook.io/brc-20-experiment/)
* Follow [@L1Fxyz 11](https://twitter.com/L1Fxyz) to keep up to date with the latest BRC-20 developments
