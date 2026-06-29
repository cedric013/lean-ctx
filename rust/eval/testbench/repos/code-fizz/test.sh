#!/bin/sh
# Unit test for fizzbuzz(): exit 0 only if every case is correct.
. ./solution.sh
[ "$(fizzbuzz 3)" = "Fizz" ] || exit 1
[ "$(fizzbuzz 5)" = "Buzz" ] || exit 1
[ "$(fizzbuzz 15)" = "FizzBuzz" ] || exit 1
[ "$(fizzbuzz 7)" = "7" ] || exit 1
[ "$(fizzbuzz 9)" = "Fizz" ] || exit 1
[ "$(fizzbuzz 10)" = "Buzz" ] || exit 1
[ "$(fizzbuzz 30)" = "FizzBuzz" ] || exit 1
exit 0
