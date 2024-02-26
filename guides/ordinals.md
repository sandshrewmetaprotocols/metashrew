# Ordinals

The following information is taken from the [Ordinal Theory Handbook](https://docs.ordinals.com/overview.html) which is recommended reading to learn about ordinals and ordinal theory.

## Ordinal Theory Overview <a href="#ordinal-theory-overview" id="ordinal-theory-overview"></a>

Ordinals are a numbering scheme for satoshis that allows tracking and transferring individual sats. These numbers are called ordinals numbers. Satoshis are numbered in the order in which they're mined, and transferred from transaction inputs to transaction outputs first-in-first-out. Both the numbering scheme and the transfer scheme rely on _order_, the numbering scheme on the _order_ in which satoshis are mined, and the transfer scheme on the _order_ of transaction inputs and outputs. Thus the name, _ordinals_.

Technical details are available in [the BIP](https://github.com/ordinals/ord/blob/master/bip.mediawiki).

Ordinal theory does not require a separate token, another blockchain, or any changes to Bitcoin. It works right now.

Ordinals numbers have a few different representations:

* _Integer notation_: [`2099994106992659`](../sandshrew/ordinals-rpc.md#sat-by-number) The ordinals number, assigned according to the order in which the satoshi was mined.
* _Decimal notation_: [`3891094.16797`](../sandshrew/ordinals-rpc.md#sat-by-decimal) The first number is the block height in which the satoshi was mined, the second the offset of the satoshi within the block.
* _Degree notation_: [`3°111094′214″16797‴`](../sandshrew/ordinals-rpc.md#sat-by-degree). We'll get to that in a moment.
* _Percentile notation_: [`99.99971949060254%`](../sandshrew/ordinals-rpc.md#sat-by-percentile) . The satoshi's position in Bitcoin's supply, expressed as a percentage.
* _Name_: [`satoshi`](../sandshrew/ordinals-rpc.md#sat-by-name). An encoding of the ordinals number using the characters `a` through `z`.

Arbitrary assets, such as NFTs, security tokens, accounts, or stablecoins can be attached to satoshis using ordinals numbers as stable identifiers.

Ordinals is an open-source project, developed [on GitHub](https://github.com/ordinals/ord). The project consists of a BIP describing the ordinals scheme, an index that communicates with a Bitcoin Core node to track the location of all satoshis, a wallet that allows making ordinals-aware transactions, a block explorer for interactive exploration of the blockchain, functionality for inscribing satoshis with digital artifacts, and this manual.

## Rarity <a href="#rarity" id="rarity"></a>

Humans are collectors, and since satoshis can now be tracked and transferred, people will naturally want to collect them. Ordinals theorists can decide for themselves which sats are rare and desirable, but there are some hints…

Bitcoin has periodic events, some frequent, some more uncommon, and these naturally lend themselves to a system of rarity. These periodic events are:

* _Blocks_: A new block is mined approximately every 10 minutes, from now until the end of time.
* _Difficulty adjustments_: Every 2016 blocks, or approximately every two weeks, the Bitcoin network responds to changes in hashrate by adjusting the difficulty target which blocks must meet in order to be accepted.
* _Halvings_: Every 210,000 blocks, or roughly every four years, the amount of new sats created in every block is cut in half.
* _Cycles_: Every six halvings, something magical happens: the halving and the difficulty adjustment coincide. This is called a conjunction, and the time period between conjunctions a cycle. A conjunction occurs roughly every 24 years. The first conjunction should happen sometime in 2032.

This gives us the following rarity levels:

* `common`: Any sat that is not the first sat of its block
* `uncommon`: The first sat of each block
* `rare`: The first sat of each difficulty adjustment period
* `epic`: The first sat of each halving epoch
* `legendary`: The first sat of each cycle
* `mythic`: The first sat of the genesis block

Which brings us to degree notation, which unambiguously represents an ordinals number in a way that makes the rarity of a satoshi easy to see at a glance:

```
A°B′C″D‴
│ │ │ ╰─ Index of sat in the block
│ │ ╰─── Index of block in difficulty adjustment period
│ ╰───── Index of block in halving epoch
╰─────── Cycle, numbered starting from 0
```

Ordinals theorists often use the terms "hour", "minute", "second", and "third" for _A_, _B_, _C_, and _D_, respectively.

Now for some examples. This satoshi is common:

```
1°1′1″1‴
│ │ │ ╰─ Not first sat in block
│ │ ╰─── Not first block in difficulty adjustment period
│ ╰───── Not first block in halving epoch
╰─────── Second cycle
```

This satoshi is uncommon:

```
1°1′1″0‴
│ │ │ ╰─ First sat in block
│ │ ╰─── Not first block in difficulty adjustment period
│ ╰───── Not first block in halving epoch
╰─────── Second cycle
```

This satoshi is rare:

```
1°1′0″0‴
│ │ │ ╰─ First sat in block
│ │ ╰─── First block in difficulty adjustment period
│ ╰───── Not the first block in halving epoch
╰─────── Second cycle
```

This satoshi is epic:

```
1°0′1″0‴
│ │ │ ╰─ First sat in block
│ │ ╰─── Not first block in difficulty adjustment period
│ ╰───── First block in halving epoch
╰─────── Second cycle
```

This satoshi is legendary:

```
1°0′0″0‴
│ │ │ ╰─ First sat in block
│ │ ╰─── First block in difficulty adjustment period
│ ╰───── First block in halving epoch
╰─────── Second cycle
```

And this satoshi is mythic:

```
0°0′0″0‴
│ │ │ ╰─ First sat in block
│ │ ╰─── First block in difficulty adjustment period
│ ╰───── First block in halving epoch
╰─────── First cycle
```

If the block offset is zero, it may be omitted. This is the uncommon satoshi from above:

```
1°1′1″
│ │ ╰─ Not first block in difficulty adjustment period
│ ╰─── Not first block in halving epoch
╰───── Second cycle
```

### Rare Satoshi Supply <a href="#rare-satoshi-supply" id="rare-satoshi-supply"></a>

#### Total Supply <a href="#total-supply" id="total-supply"></a>

* `common`: 2.1 quadrillion
* `uncommon`: 6,929,999
* `rare`: 3437
* `epic`: 32
* `legendary`: 5
* `mythic`: 1

#### Current Supply <a href="#current-supply" id="current-supply"></a>

* `common`: 1.9 quadrillion
* `uncommon`: 808,262
* `rare`: 369
* `epic`: 3
* `legendary`: 0
* `mythic`: 1

At the moment, even uncommon satoshis are quite rare. As of this writing, 745,855 uncommon satoshis have been mined - one per 25.6 bitcoin in circulation.

## Names <a href="#names" id="names"></a>

Each satoshi has a name, consisting of the letters _A_ through _Z_, that get shorter the further into the future the satoshi was mined. They could start short and get longer, but then all the good, short names would be trapped in the unspendable genesis block.

As an example, 1905530482684727°'s name is "iaiufjszmoba". The name of the last satoshi to be mined is "a". Every combination of 10 characters or less is out there, or will be out there, someday.

## Exotics <a href="#exotics" id="exotics"></a>

Satoshis may be prized for reasons other than their name or rarity. This might be due to a quality of the number itself, like having an integer square or cube root. Or it might be due to a connection to a historical event, such as satoshis from block 477,120, the block in which SegWit activated, or 2099999997689999°, the last satoshi that will ever be mined.

Such satoshis are termed "exotic". Which satoshis are exotic and what makes them so is subjective. Ordinals theorists are encouraged to seek out exotics based on criteria of their own devising.

## Inscriptions <a href="#inscriptions" id="inscriptions"></a>

Satoshis can be inscribed with arbitrary content, creating Bitcoin-native digital artifacts. Inscribing is done by sending the satoshi to be inscribed in a transaction that reveals the inscription content on-chain. This content is then inextricably linked to that satoshi, turning it into an immutable digital artifact that can be tracked, transferred, hoarded, bought, sold, lost, and rediscovered.

More information can be found in the [Inscriptions Guide](ordinals.md#inscriptions).
