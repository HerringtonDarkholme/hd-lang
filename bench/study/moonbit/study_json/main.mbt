// json: encode records to JSON text, decode them back, check the sums.

///|
let n : Int32 = 170_000

///|
struct Record {
  id : Int32
  name : String
  active : Bool
  score : Int32
  tags : Array[String]
} derive(ToJson, @json.FromJson)

///|
fn main {
  let records : Array[Record] = []
  for i in 0..<n {
    records.push({
      id: i,
      name: "user\{i}",
      active: i % 3 == 0,
      score: i * 37 % 1000,
      tags: ["t\{i % 10}", "g\{i % 7}"],
    })
  }
  let text = Json(records).stringify()
  let back : Array[Record] = @json.from_json(@json.parse(text)) catch {
    e => abort("decode: \{e}")
  }
  let mut sum : Int64 = 0
  for r in back {
    sum = sum +
      r.score.to_int64() +
      r.tags[1].length().to_int64() +
      (if r.active { 1L } else { 0L })
  }
  println("\{text.length()} \{sum}")
}
