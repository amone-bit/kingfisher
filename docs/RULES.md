# Writing Custom Rules for Kingfisher

A _rule_ in Kingfisher is a YAML document that describes how to detect and (optionally) validate secrets in your codebase. With custom rules you can:

- **Extend** Kingfisher without touching Rust code  
- **Tune** sensitivity via entropy and confidence  
- **Plug in** live checks against external services  

This document explains how to write custom rules for Kingfisher using a YAML-based rule system. The rules define regular expressions to detect secrets in source code and other textual data, and they can include validation steps to confirm the secret's authenticity. By using a rules-based system, Kingfisher is highly extensible—new rules can be added or existing ones modified without changing the core code.

## 1. Rule Schema

Each rule file defines one or more entries under a top‑level `rules:` list. Every entry supports the following fields:

```yaml
rules:
  - name:           # (string) Human-friendly rule name
    id:             # (string) Unique identifier (e.g. kingfisher.aws.1)

    pattern: |      # (multi-line regex) Detection pattern
      (?x)(?i)
      aws
      (?:.|[\n\r]){0,32}?
      \b([A-Za-z0-9/+=]{40})\b

    min_entropy: 3.5                # (float) Minimum Shannon entropy
    confidence:  medium             # (enum: low | medium | high)

    examples:                       # (list) strings that must match
      - AWS_SECRET="AKIA…"

    references:                     # (optional list) context URLs
      - https://docs.aws.amazon.com/general/latest/gr/aws-sec-cred-types.html

    visible: true                   # (bool) hide helper matches when false

    depends_on_rule:                # (optional) capture chaining
      - rule_id: kingfisher.aws.id
        variable: AKID              # referenced as {{ AKID }}

    validation:                     # (optional) live validation
      type: Http
      content:
        request:
          method: GET
          url: https://api.example.com/v1/check
          headers:
            X-Secret: "{{ TOKEN }}"
            X-Id:     "{{ AKID }}"
          response_is_html: true # by default, validation responses containing HTML or considered invalid. Set to `true` if you expect HTML returned from a validation response
          response_matcher:
            - report_response: true   # always include raw payload
            - type: StatusMatch
              status: [200]           # positive check
            - type: StatusMatch
              status: [401,403]
              negative: true          # negative check → must NOT match
            - type: HeaderMatch
              header: content-type
              expected: ["application/json"]
            - type: JsonValid
```

| Field             | What it does                                                         |
| ----------------- | -------------------------------------------------------------------- |
| name              | Friendly name shown in reports                                       |
| id                | Unique text ID (namespace.v#) used internally                        |
| pattern           | Regex used to spot secrets (free‑spacing & flags allowed)            |
| min_entropy       | Threshold to guard against low‑complexity false positives            |
| confidence        | Suggests severity: low → high                                        |
| examples          | Good matches; used for testing                                       |
| visible           | false to hide non‑secret captures (e.g. IDs)                         |
| depends_on_rule   | Chain rules: use captures from one rule in another’s validation      |
| validation        | Configure HTTP, AWS, GCP, etc. checks to verify live validity        |


*responser_matcher* variants. Multiple can be used
| Variant         | Required keys                                                                                              | Behavior                                                                |
|-----------------|-------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------|
| **StatusMatch** | `status` (list\<int>)<br>`negative` (bool, default `false`)                                                 | Pass when codes match (or don’t match if `negative`).                     |
| **WordMatch**   | `words` (list\<string>)<br>`match_all_words` (bool)<br>`negative` (bool)                                    | Word/substring checks in body.                                            |
| **HeaderMatch** | `header` (string)<br>`expected` (list\<string>)<br>`match_all_values` (bool)                                | Header value assertions.                                                  |
| **JsonValid**   | –                                                                                                           | Pass only if body parses as JSON. Use when response is expected as JSON data                                       |
| **XmlValid**    | –                                                                                                           | Pass only if body parses as well-formed XML. Use when response is expected as XML data                             |
| **ReportResponse** | `report_response` (bool)                                                                                | Include raw payload in finding for debugging.                             |

## 2. Templating with Liquid
Kingfisher leverages the Liquid template engine for dynamic parts of HTTP request bodies, headers, query parameters, and multipart payloads. The engine supports both built-in and custom filters to manipulate the captured secret (TOKEN) or other named captures ({{ NAME }}).

### Using Liquid Filters in Validation
- **Capture Injection**: The unnamed capture from your regex becomes {{ TOKEN }}. Named captures are made available as uppercase variables (e.g. {{ RDMVAL }}).
- **Filter Pipeline**: You can chain filters using the pipe (|) syntax:

```liquid
{{ TOKEN | b64enc | url_encode }}
```
Arguments: Some filters accept parameters, provided after a colon:

```liquid
{{ TOKEN | hmac_sha256: "my-secret-key" }}
```

### 3. Built-in & Custom Liquid Filters

Below is the complete list of Liquid filters available in Kingfisher, along with their usage patterns and examples.
| Filter                | Parameters                                   | Description                                                                                                    | Example                                                             |
| --------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `b64enc`              | –                                            | Base64-encodes the input using the standard alphabet.                                                          | `{{ TOKEN \| b64enc }}`                                              |
| `b64url_enc`          | –                                            | URL-safe Base64 (no padding). Useful for JWT headers & payloads.                                               | `{{ TOKEN \| b64url_enc }}`                                          |
| `sha256`              | –                                            | Computes the SHA-256 hex digest of the input.                                                                  | `{{ TOKEN \| sha256 }}`                                              |
| `hmac_sha1`           | `key` (string)                               | Computes HMAC-SHA1 over the input, returns Base64-encoded result.                                              | `{{ TOKEN \| hmac_sha1: "secret-key" }}`                             |
| `hmac_sha256`         | `key` (string)                               | Computes HMAC-SHA256 over the input, returns Base64-encoded result.                                            | `{{ TOKEN \| hmac_sha256: "secret-key" }}`                           |
| `hmac_sha384`         | `key` (string)                               | Computes HMAC-SHA384 over the input, returns Base64-encoded result.                                            | `{{ TOKEN \| hmac_sha384: "secret-key" }}`                           |
| `random_string`       | `len` (integer, optional)                    | Generates a cryptographically-secure random alphanumeric string of the specified length (default: 32).        | `{{ "" \| random_string: 16 }}`                                      |
| `url_encode`          | –                                            | Percent-encodes the input according to RFC 3986.                                                                | `{{ TOKEN \| url_encode }}`                                          |
| `json_escape`         | –                                            | Escapes special characters so a string can be safely injected into JSON contexts.                              | `{{ TOKEN \| json_escape }}`                                         |
| `unix_timestamp`      | –                                            | Returns the current Unix epoch time in seconds (UTC).                                                          | `{{ "" \| unix_timestamp }}`                                         |
| `iso_timestamp`       | –                                            | Returns the current UTC timestamp in full ISO-8601 format (may include fractional seconds).                    | `{{ "" \| iso_timestamp }}`                                          |
| `iso_timestamp_no_frac` | –                                          | Current ISO-8601 timestamp (UTC) **without** fractional seconds.                                               | `{{ "" \| iso_timestamp_no_frac }}`                                  |
| `uuid`                | –                                            | Generates a random UUIDv4 string.                                                                              | `{{ "" \| uuid }}`                                                   |
| `jwt_header`          | –                                            | Builds a minimal JWT header JSON (`{"typ":"JWT","alg":…}`) and Base64URL-encodes it.                           | `{{ "HS256" \| jwt_header }}`                                        |
| `replace`             | `from` (string), `to` (string)               | Replaces every occurrence of `from` with `to` in the input string.                                             | `{{ "hello world" \| replace: "world", "mars" }}`                    |


**Chaining & Composition:** Filters can be stacked; e.g.:

```liquid
Authorization: Basic {{ "api:" | append: TOKEN | b64enc }}
```

**Runtime Values:** Filters like unix_timestamp and uuid are evaluated at runtime, enabling nonces, timestamps, and unique IDs in your requests.
### How depends_on_rule Works

- **Dependency Declaration:**  
  In your YAML rule definition, you add a `depends_on_rule` section. Here you specify:
  - **rule_id:** The identifier of the rule whose output is required.
  - **variable:** The name (typically in uppercase) that will be used to reference the captured value from the dependency rule.

- **Chaining Captures:**  
  When Kingfisher scans a file, it processes rules in a specific order. If a rule has a dependency, the engine first checks whether the dependent rule has already matched on the same input (or blob). If it did, the captured value (for example, an access key ID) is made available to the dependent rule.

- **Using the Captured Value:**  
  This captured value can then be used during the validation phase. For instance, if you have a rule for an Algolia Admin API Key that depends on an Algolia Application ID (captured as `APPID`), the validation logic can incorporate the `APPID` value to confirm that the secret matches the expected pattern or format for that specific account.

### Use depends_on_rule to require one rule before another runs:

```yaml
depends_on_rule:
  - rule_id: kingfisher.algolia.app_id   # must match first
    variable: APPID                     # captured as {{ APPID }}
```

- **Capture flow**: First rule captures `APPID` → second rule injects `{{ APPID }}` into validation HTTP request or pattern
- **Visible control:** set `visible: false` on the supporting rule so it doesn’t clutter your report for non-secret matches
## Algolia Example

Consider this example rule for an Algolia Application ID and Admin Key combination. To validate that this is an active credential, both must be matched:

```yaml
rules:
  - name: Algolia Admin API Key
    id: kingfisher.algolia.1
    pattern: |
      (?xi)
      algolia
      (?:.|[\n\r]){0,32}?
      \b
      (
        [a-z0-9]{32}
      )
      \b
    min_entropy: 3.5
    confidence: medium
    examples:
      - algolia_api_key = "ij1mut5oe606wlrf5z4u8u31264z3gag"
    validation:
      type: Http
      content:
        request:
          headers:
            X-Algolia-API-Key: '{{ TOKEN }}'
            X-Algolia-Application-Id: '{{ APPID }}'
          method: GET
          response_matcher:
            - report_response: true
            - status:
                - 200
              type: StatusMatch
          url: https://{{ APPID }}-dsn.algolia.net/1/keys
    depends_on_rule:
      - rule_id: "kingfisher.algolia.2"
        variable: APPID
  
  - name: Algolia Application ID
    id: kingfisher.algolia.2
    pattern: |
      (?xi)
      algolia
      (?:.|[\n\r]){0,16}?
      \b
      (
        [A-Z0-9]{10}
      )
      \b               
    min_entropy: 3.5
    visible: false
    confidence: medium
    examples:
      - algolia_app_id = "WRB8YLFW7Y"

```

### How It Works:

* Algolia Application ID Rule (kingfisher.algolia.2):

  This rule scans for an Algolia Application ID—a 10-character alphanumeric string. It is marked with visible: false so that even if it matches, the finding is not directly reported. Its primary role is to provide a supporting value for other rules rather than to be flagged as a secret by itself.

* Algolia Admin API Key Rule (kingfisher.algolia.1):
  This rule detects the Algolia Admin API Key using a regex pattern. It includes a depends_on_rule property that specifies a dependency on the Algolia Application ID rule.

  * The dependency declares that the rule requires the output of the Algolia Application ID rule, and the captured value is assigned to the variable APPID.
  * In the validation section, this captured `APPID` is used dynamically in the HTTP request (for example, in the header `X-Algolia-Application-Id` and in the URL).

The dependency mechanism (depends_on_rule) ensures that:

* Non-secret data (like an application ID) is captured without cluttering the scan report (thanks to visible: false).
* The secret (the API key) is validated in context, with the necessary supporting information automatically injected.
* Rules remain modular and extensible; you can update the dependent rule or its pattern independently, and the change will automatically be reflected where the value is used.

## The `visible: false` Property

The `visible: false` property tells Kingfisher to hide the finding from the final scan report. This is particularly useful for rules that capture data not meant to be reported as a secret, but rather to serve as supporting context for another rule.

For example, a rule might match a username, an email address, an AWS Access Key ID, or an Application ID. While these pieces of information are captured during scanning, they are not secrets on their own. Instead, they are used by other rules—via the `depends_on_rule` mechanism—to validate an associated secret. By marking such rules as `visible: false`, you prevent these non-secret findings from cluttering your report, yet their values remain available for dependent rules.

`visible: false` helps keep the scan output focused on actual secrets while still capturing important contextual data needed for comprehensive validation.


## Writing Custom Rules

When writing custom rules, consider the following best practices:

1. **Multi-line Regex:** Write your regex patterns over multiple lines for clarity. Use the `(?x)` flag to enable free-spacing mode.
2. **Optimize for Performance:** Structure your regex to minimize backtracking. Use non-capturing groups where possible and keep the pattern as concise as possible.
3. **Validation Integration:** Define a `validation` section if you want to verify the detected secret. You can use Liquid templating to insert dynamic values—use the unnamed capture as `TOKEN` and any named captures in uppercase.
4. **Test with Examples:** Always include examples that should match and, optionally, negative examples to ensure your rule behaves as expected.

## Examples

Below are some examples to guide you in writing custom rules

### Anthropic API Key

```yaml
rules:
  - name: Anthropic API Key
    id: kingfisher.anthropic.1
    pattern: |
      (?xi)                    
      \b                       
      (                        
        sk-ant-api
        \d{2,4}
        -
        [\w\-]{93}
        AA
      )                        
      \b                       
    min_entropy: 3.3
    confidence: medium
    examples:
      - sk-ant-api668-Clm512odot9WDD7itfUU9R880nefA1EtYZDbpE-C9b0XQEWpqFKf9DQUo03vOfXl16oSmyar1CLF1SzV3YzpZJ6bahcpLAA
    categories:
      - api
      - secret
    references:
      - https://docs.anthropic.com/claude/reference/authentication
    validation:
      type: Http
      content:
        request:
          body: |
            {
              "model": "claude-3-haiku-20240307",
              "max_tokens": 1024,
              "messages": [
                {"role": "user", "content": "respond only with 'success'"}
              ]
            }
          headers:
            Content-Type: application/json
            anthropic-version: "2023-06-01"
            x-api-key: '{{ TOKEN }}'
          method: POST
          response_matcher:
            - report_response: true
            - status:
                - 200
              type: StatusMatch
            - report_response: true
            - type: WordMatch
              words:
                - '"type":"invalid_request_error"'
          url: https://api.anthropic.com/v1/messages
```

### FileIO Secret Key
```yaml
rules:
  - name: FileIO Secret Key
    id: kingfisher.fileio.1
    pattern: |
      (?xi)
      \b
      fileio
      (?:.|[\n\r]){0,32}?
      (?:SECRET|PRIVATE|ACCESS|KEY|TOKEN)
      (?:.|[\n\r]){0,16}?
      \b
      (
        [A-Z0-9]{16}
        (?:\.[A-Z0-9]{7}){2}
        \.[A-Z0-9]{8}
      )
      \b
    min_entropy: 3.3
    confidence: medium
    examples:
      - fileio SECRETKEY = Z9Y8X7W6V5U4T3S2R1Q0.P9O8N7M6L5K4J3H2G1F
      - fileio.PRIVATE.TOKEN = F0E1D2C3B4A596877869.5E4D3C2B1A0Z9Y8X7W6V
      - fileio_key = M8N6B4V2C0X9Z7L5K3J1.H2G4F6D8S0A9P7O5I3U1
    validation:
      type: Http
      content:
        request:
          method: GET
          url: https://file.io/api/v2/account
          headers:
            Authorization: "Bearer {{ TOKEN }}"
          response_matcher:
            - report_response: true
            - type: StatusMatch
              status: [200]
            - type: HeaderMatch
              header: content-type
              expected: ["application/json"]
            - type: JsonValid

```

## Advanced Example

This advanced example uses the liquid-rs filters included with Kingfisher to sign a request to validate Alibaba Cloud credential pairs:

```yaml
rules:
  - name: Alibaba Access Key ID
    id: kingfisher.alibabacloud.1
    pattern: |
      (?xi)
      \b
      (
        LTAI[a-z0-9]{17,21}
      )
      \b
    min_entropy: 4.0
    confidence: medium
    visible: false
    examples:
      - LTAI8x2NiGqfyJGx7eLDhp12
      - LTAI5GqyJGhp12ad31L5hpix
  - name: Alibaba Access Key Secret
    id: kingfisher.alibabacloud.2
    pattern: |
      (?xi)
      \b
      alibaba
      (?:.|[\n\r]){0,32}?
      \b
      (
        [a-z0-9]{30}
      )
      \b
    min_entropy: 4.2
    confidence: medium
    examples:
      - alibaba_secret = 7jkWdTjKLnSlGddwPR5gBn65PHcZG6
      - alibaba-token = aJHKLnSlGddwPR5g7jkWdTBn65PHc5
    validation:
      type: Http
      content:
        request:
          method: GET
          url: >
            {%- assign nonce = "" | uuid | upcase -%}
            {%- assign raw_timestamp = "" | iso_timestamp_no_frac -%}
            {%- assign timestamp = raw_timestamp | replace: ":", "%3A" -%}

            {%- capture params -%}
            AccessKeyId={{ AKID | url_encode }}&Action=GetCallerIdentity&Format=JSON&SignatureMethod=HMAC-SHA1&SignatureNonce={{ nonce }}&SignatureVersion=1.0&Timestamp={{ timestamp }}&Version=2015-04-01
            {%- endcapture -%}
            {%- assign encoded_params = params | replace: "+", "%20" | replace: "*", "%2A" | replace: "%7E", "~" -%}
            {%- assign query_string = encoded_params | url_encode | replace: "%2D", "-" | replace: "%2E", "." -%}
            
            {%- assign signature_base_string = "GET&%2F&" | append: query_string -%}
            {%- assign token_amp = TOKEN | append: "&" -%}

            {%- assign hmacsignature = signature_base_string | hmac_sha1: token_amp | url_encode -%}

            https://sts.aliyuncs.com/?{{ params }}&Signature={{ hmacsignature }}
          headers:
            Accept: application/json
          response_matcher:
            - report_response: true
            - type: StatusMatch
              status: [200]
            - type: WordMatch
              words: ['"Arn"']
    depends_on_rule:
      - rule_id: kingfisher.alibabacloud.1
        variable: AKID```