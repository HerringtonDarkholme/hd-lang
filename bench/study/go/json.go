// json: encode 400k records to JSON text, decode it back, check the sums.
package main

import (
	"encoding/json"
	"fmt"
)

const n = 400_000

type record struct {
	ID     int64    `json:"id"`
	Name   string   `json:"name"`
	Active bool     `json:"active"`
	Score  int64    `json:"score"`
	Tags   []string `json:"tags"`
}

func main() {
	records := make([]record, 0, n)
	for i := 0; i < n; i++ {
		records = append(records, record{int64(i), fmt.Sprintf("user%d", i), i%3 == 0, int64(i * 37 % 1000),
			[]string{fmt.Sprintf("t%d", i%10), fmt.Sprintf("g%d", i%7)}})
	}
	text, err := json.Marshal(records)
	if err != nil {
		panic(err)
	}
	var back []record
	if err := json.Unmarshal(text, &back); err != nil {
		panic(err)
	}
	var sum int64
	for _, r := range back {
		sum += r.Score + int64(len(r.Tags[1]))
		if r.Active {
			sum++
		}
	}
	fmt.Println(len(text), sum)
}
