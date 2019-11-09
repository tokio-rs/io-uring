mod ring;

use loom::sync::Arc;
use loom::thread;
use ring::Ring;


fn main() {
    loom::model(|| {
        let ring = Arc::new(Ring::<u32>::new());

        let mut joins = Vec::new();
        for x in 0..2 {
            let ring = ring.clone();
            let j = thread::spawn(move || {
                for y in 0..2 {
                    assert!(ring.push(x + y).is_ok());
                }

                while let Some(_z) = ring.pop() {}
            });
            joins.push(j);
        }

        for j in joins {
            j.join().unwrap();
        }
    });
}
