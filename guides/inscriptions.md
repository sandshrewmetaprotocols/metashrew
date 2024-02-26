# Inscriptions

The following information is taken from the [Ordinal Theory Handbook](https://docs.ordinals.com/overview.html) which is recommended reading to learn about ordinals and ordinal theory.

## Overview

Inscriptions inscribe sats with arbitrary content, creating bitcoin-native digital artifacts, more commonly known as NFTs. Inscriptions do not require a sidechain or separate token.

These inscribed sats can then be transferred using bitcoin transactions, sent to bitcoin addresses, and held in bitcoin UTXOs. These transactions, addresses, and UTXOs are normal bitcoin transactions, addresses, and UTXOS in all respects, with the exception that in order to send individual sats, transactions must control the order and value of inputs and outputs according to ordinal theory.

The inscription content model is that of the web. An inscription consists of a content type, also known as a MIME type, and the content itself, which is a byte string. This allows inscription content to be returned from a web server, and for creating HTML inscriptions that use and remix the content of other inscriptions.

Inscription content is entirely on-chain, stored in taproot script-path spend scripts. Taproot scripts have very few restrictions on their content, and additionally receive the witness discount, making inscription content storage relatively economical.

Since taproot script spends can only be made from existing taproot outputs, inscriptions are made using a two-phase commit/reveal procedure. First, in the commit transaction, a taproot output committing to a script containing the inscription content is created. Second, in the reveal transaction, the output created by the commit transaction is spent, revealing the inscription content on-chain.

Inscription content is serialized using data pushes within unexecuted conditionals, called "envelopes". Envelopes consist of an `OP_FALSE OP_IF … OP_ENDIF` wrapping any number of data pushes. Because envelopes are effectively no-ops, they do not change the semantics of the script in which they are included, and can be combined with any other locking script.

A text inscription containing the string "Hello, world!" is serialized as follows:

```
OP_FALSE
OP_IF
  OP_PUSH "ord"
  OP_PUSH 1
  OP_PUSH "text/plain;charset=utf-8"
  OP_PUSH 0
  OP_PUSH "Hello, world!"
OP_ENDIF
```

First the string `ord` is pushed, to disambiguate inscriptions from other uses of envelopes.

`OP_PUSH 1` indicates that the next push contains the content type, and `OP_PUSH 0`indicates that subsequent data pushes contain the content itself. Multiple data pushes must be used for large inscriptions, as one of taproot's few restrictions is that individual data pushes may not be larger than 520 bytes.

The inscription content is contained within the input of a reveal transaction, and the inscription is made on the first sat of its input. This sat can then be tracked using the familiar rules of ordinal theory, allowing it to be transferred, bought, sold, lost to fees, and recovered.

## Content

The data model of inscriptions is that of a HTTP response, allowing inscription content to be served by a web server and viewed in a web browser.

## Fields

Inscriptions may include fields before an optional body. Each field consists of two data pushes, a tag and a value.

Currently, there are six defined fields:

* `content_type`, with a tag of `1`, whose value is the MIME type of the body.
* `pointer`, with a tag of `2`, see pointer docs.
* `parent`, with a tag of `3`, see provenance.
* `metadata`, with a tag of `5`, see metadata.
* `metaprotocol`, with a tag of `7`, whose value is the metaprotocol identifier.
* `content_encoding`, with a tag of `9`, whose value is the encoding of the body.
* `delegate`, with a tag of `11`, see delegate.

The beginning of the body and end of fields is indicated with an empty data push.

Unrecognized tags are interpreted differently depending on whether they are even or odd, following the "it's okay to be odd" rule used by the Lightning Network.

Even tags are used for fields which may affect creation, initial assignment, or transfer of an inscription. Thus, inscriptions with unrecognized even fields must be displayed as "unbound", that is, without a location.

Odd tags are used for fields which do not affect creation, initial assignment, or transfer, such as additional metadata, and thus are safe to ignore.

## Inscription IDs

The inscriptions are contained within the inputs of a reveal transaction. In order to uniquely identify them they are assigned an ID of the form:

`521f8eccffa4c41a3a7728dd012ea5a4a02feed81f41159231251ecf1e5c79dai0`

The part in front of the `i` is the transaction ID (`txid`) of the reveal transaction. The number after the `i` defines the index (starting at 0) of new inscriptions being inscribed in the reveal transaction.

Inscriptions can either be located in different inputs, within the same input or a combination of both. In any case the ordering is clear, since a parser would go through the inputs consecutively and look for all inscription `envelopes`.

| Input | Inscription Count |   Indices  |
| :---: | :---------------: | :--------: |
|   0   |         2         |   i0, i1   |
|   1   |         1         |     i2     |
|   2   |         3         | i3, i4, i5 |
|   3   |         0         |            |
|   4   |         1         |     i6     |

## Sandboxing

HTML and SVG inscriptions are sandboxed in order to prevent references to off-chain content, thus keeping inscriptions immutable and self-contained.

This is accomplished by loading HTML and SVG inscriptions inside `iframes` with the `sandbox` attribute, as well as serving inscription content with `Content-Security-Policy` headers.

## Delegate Inscriptions

Inscriptions may nominate a delegate inscription. Requests for the content of an inscription with a delegate will instead return the content and content type of the delegate. This can be used to cheaply create copies of an inscription.

#### Specification

To create an inscription I with delegate inscription D:

* Create an inscription D. Note that inscription D does not have to exist when making inscription I. It may be inscribed later. Before inscription D is inscribed, requests for the content of inscription I will return a 404.
* Include tag `11`, i.e. `OP_PUSH 11`, in I, with the value of the serialized binary inscription ID of D, serialized as the 32-byte `TXID`, followed by the four-byte little-endian `INDEX`, with trailing zeroes omitted.

_Note: The bytes of a bitcoin transaction ID are reversed in their text representation, so the serialized transaction ID will be in the opposite order._

#### Example

An example of an inscription which delegates to `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0`:

```
OP_FALSE
OP_IF
  OP_PUSH "ord"
  OP_PUSH 11
  OP_PUSH 0x1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100
OP_ENDIF
```

Note that the value of tag `11` is binary, not hex.

The delegate field value uses the same encoding as the parent field. See provenance for more examples of inscrpition ID encodings;

## Metadata

Inscriptions may include [CBOR](https://cbor.io/) metadata, stored as data pushes in fields with tag `5`. Since data pushes are limited to 520 bytes, metadata longer than 520 bytes must be split into multiple tag `5` fields, which will then be concatenated before decoding.

Metadata is human readable, and all metadata will be displayed to the user with its inscription. Inscribers are encouraged to consider how metadata will be displayed, and make metadata concise and attractive.

Metadata is rendered to HTML for display as follows:

* `null`, `true`, `false`, numbers, floats, and strings are rendered as plain text.
* Byte strings are rendered as uppercase hexadecimal.
* Arrays are rendered as `<ul>` tags, with every element wrapped in `<li>` tags.
* Maps are rendered as `<dl>` tags, with every key wrapped in `<dt>` tags, and every value wrapped in `<dd>` tags.
* Tags are rendered as the tag , enclosed in a `<sup>` tag, followed by the value.

CBOR is a complex spec with many different data types, and multiple ways of representing the same data. Exotic data types, such as tags, floats, and bignums, and encoding such as indefinite values, may fail to display correctly or at all. Contributions to `ord` to remedy this are welcome.

### Example

Since CBOR is not human readable, in these examples it is represented as JSON. Keep in mind that this is _only_ for these examples, and JSON metadata will _not_ be displayed correctly.

The metadata `{"foo":"bar","baz":[null,true,false,0]}` would be included in an inscription as:

```
OP_FALSE
OP_IF
    ...
    OP_PUSH 0x05 OP_PUSH '{"foo":"bar","baz":[null,true,false,0]}'
    ...
OP_ENDIF
```

And rendered as:

```
<dl>
  ...
  <dt>metadata</dt>
  <dd>
    <dl>
      <dt>foo</dt>
      <dd>bar</dd>
      <dt>baz</dt>
      <dd>
        <ul>
          <li>null</li>
          <li>true</li>
          <li>false</li>
          <li>0</li>
        </ul>
      </dd>
    </dl>
  </dd>
  ...
</dl>
```

Metadata longer than 520 bytes must be split into multiple fields:

```
OP_FALSE
OP_IF
    ...
    OP_PUSH 0x05 OP_PUSH '{"very":"long","metadata":'
    OP_PUSH 0x05 OP_PUSH '"is","finally":"done"}'
    ...
OP_ENDIF
```

Which would then be concatenated into `{"very":"long","metadata":"is","finally":"done"}`.

## Pointer

In order to make an inscription on a sat other than the first of its input, a zero-based integer, called the "pointer", can be provided with tag `2`, causing the inscription to be made on the sat at the given position in the outputs. If the pointer is equal to or greater than the number of total sats in the outputs of the inscribe transaction, it is ignored, and the inscription is made as usual. The value of the pointer field is a little endian integer, with trailing zeroes ignored.

An even tag is used, so that old versions of `ord` consider the inscription to be unbound, instead of assigning it, incorrectly, to the first sat.

This can be used to create multiple inscriptions in a single transaction on different sats, when otherwise they would be made on the same sat.

### Examples

An inscription with pointer 255:

```
OP_FALSE
OP_IF
  OP_PUSH "ord"
  OP_PUSH 1
  OP_PUSH "text/plain;charset=utf-8"
  OP_PUSH 2
  OP_PUSH 0xff
  OP_PUSH 0
  OP_PUSH "Hello, world!"
OP_ENDIF
```

An inscription with pointer 256:

```
OP_FALSE
OP_IF
  OP_PUSH "ord"
  OP_PUSH 1
  OP_PUSH "text/plain;charset=utf-8"
  OP_PUSH 2
  OP_PUSH 0x0001
  OP_PUSH 0
  OP_PUSH "Hello, world!"
OP_ENDIF
```

An inscription with pointer 256, with trailing zeroes, which are ignored:

```
OP_FALSE
OP_IF
  OP_PUSH "ord"
  OP_PUSH 1
  OP_PUSH "text/plain;charset=utf-8"
  OP_PUSH 2
  OP_PUSH 0x000100
  OP_PUSH 0
  OP_PUSH "Hello, world!"
OP_ENDIF
```

## Provenance

The owner of an inscription can create child inscriptions, trustlessly establishing the provenance of those children on-chain as having been created by the owner of the parent inscription. This can be used for collections, with the children of a parent inscription being members of the same collection.

Children can themselves have children, allowing for complex hierarchies. For example, an artist might create an inscription representing themselves, with sub inscriptions representing collections that they create, with the children of those sub inscriptions being items in those collections.

#### Specification

To create a child inscription C with parent inscription P:

* Create an inscribe transaction T as usual for C.
* Spend the parent P in one of the inputs of T.
* Include tag `3`, i.e. `OP_PUSH 3`, in C, with the value of the serialized binary inscription ID of P, serialized as the 32-byte `TXID`, followed by the four-byte little-endian `INDEX`, with trailing zeroes omitted.

_NB_ The bytes of a bitcoin transaction ID are reversed in their text representation, so the serialized transaction ID will be in the opposite order.

#### Example

An example of a child inscription of `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0`:

```
OP_FALSE
OP_IF
  OP_PUSH "ord"
  OP_PUSH 1
  OP_PUSH "text/plain;charset=utf-8"
  OP_PUSH 3
  OP_PUSH 0x1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100
  OP_PUSH 0
  OP_PUSH "Hello, world!"
OP_ENDIF
```

Note that the value of tag `3` is binary, not hex, and that for the child inscription to be recognized as a child, `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0` must be spent as one of the inputs of the inscribe transaction.

Example encoding of inscription ID `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi255`:

```
OP_FALSE
OP_IF
  …
  OP_PUSH 3
  OP_PUSH 0x1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100ff
  …
OP_ENDIF
```

And of inscription ID `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi256`:

```
OP_FALSE
OP_IF
  …
  OP_PUSH 3
  OP_PUSH 0x1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a090807060504030201000001
  …
OP_ENDIF
```

#### Notes

The tag `3` is used because it is the first available odd tag. Unrecognized odd tags do not make an inscription unbound, so child inscriptions would be recognized and tracked by old versions of `ord`.

A collection can be closed by burning the collection's parent inscription, which guarantees that no more items in the collection can be issued.

## Recursion

[Recursive inscriptions](../sandshrew/ordinals-rpc.md#recursive-methods), as conceptualized within Casey Rodarmor's ordinals framework, present an important exception to inscription sandboxing limitations by facilitating the access and incorporation of existing inscription data into new inscriptions.

By leveraging recursive inscriptions, users can effectively 'daisy-chain' data, pulling information from a series of interconnected inscriptions. This approach has the potential to support increasingly sophisticated applications, ranging from intricate smart contracts and interactive video games to diverse forms of digital innovation. It essentially allows the creation of a network of interrelated data sources, mitigating the constraints imposed by the 4 MB block space limit.

This advanced functionality of recursive inscriptions is made accessible through Ordinals Indexers like Sandshrew. These indexers employ a complex call mechanism to retrieve and integrate recursive inscription data, enhancing the dynamism and interactivity within the Bitcoin blockchain ecosystem. This innovation not only expands the scope of blockchain applications but also illustrates the evolving nature of Bitcoin's capabilities beyond mere financial transactions.

Refer to the [Sandshrew Ordinals RPC Recursion Methods](../sandshrew/ordinals-rpc.md#recursive-methods) for more information.
