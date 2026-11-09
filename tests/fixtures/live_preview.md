# Live Preview fixture

## Heading with *emphasis* and a closing sequence ##

Setext heading
--------------

A paragraph with **bold**, *italic*, ***both***, ~~struck~~, `inline code`,
``code with ` tick``, an [inline link](https://example.com "Title"), a
[reference link][ref], a [collapsed][] link, a [shortcut] link, an
![image](img.png), and <https://example.org>. Escapes \*stay\* literal and
entities like &amp; and &#x263A; decode. Nested **bold with *italic* and
`code` inside** keeps both styles. Unicode: Բարև աշխարհ, 日本語, café, 🙂.
This sentence is deliberately long so that it wraps across several visual lines in a narrow window and exercises hanging indentation and caret placement across wrapped text.
Hard break here  
and after a backslash\
too.

```rust title="main.rs"
fn main() {
    println!("Բարև");
}
```

~~~
tilde fence without a language
~~~

> A quote line with **bold**
> continues here
lazily continued
>
> > A nested quote
> > - with a list item

- First bullet
- Second bullet with a long item that wraps onto another visual line so its hanging indent can be checked carefully
  and a continuation line
* Star bullet
+ Plus bullet

3. Three, rendered as the first number
7. Seven, rendered as the second
9) Nine with a parenthesis

- [ ] Open task
- [x] Done task with `code`
- [ ] Task with nested items
  - [x] Nested done task
  1. Nested ordered item

- Loose item one

- Loose item two

  > A quote inside a list item

  ```js
  const inList = true;
  ```

[ref]: https://example.com/ref
[collapsed]: https://example.com/collapsed
[shortcut]: https://example.com/shortcut
