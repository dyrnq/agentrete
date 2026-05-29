use std::time::Instant;
use model2vec_rs::model::StaticModel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading /tmp/m2v-bge-small-zh ...");
    let t = Instant::now();
    let model = StaticModel::from_pretrained("/tmp/m2v-bge-small-zh", None, None, None)?;
    println!("Loaded in {:.1}s", t.elapsed().as_secs_f64());

    let emb = model.encode_single("你好世界");
    println!("Single: dim={}", emb.len());

    let texts_100: Vec<String> = (0..100).map(|_| "测试中文嵌入".into()).collect();
    let t = Instant::now();
    let _ = model.encode(&texts_100);
    println!("100 texts: {:.3}ms ({:.2}ms each)", t.elapsed().as_secs_f64()*1000.0, t.elapsed().as_secs_f64()*10.0);

    let texts_500: Vec<String> = (0..500).map(|_| "Rust异步sqlite向量搜索".into()).collect();
    let t = Instant::now();
    let _ = model.encode(&texts_500);
    println!("500 texts: {:.3}ms ({:.2}ms each)", t.elapsed().as_secs_f64()*1000.0, t.elapsed().as_secs_f64()*2.0);

    let texts_1000: Vec<String> = (0..1000).map(|_| "长文本测试: Rust agent memory system with sqlite-vec KNN search".into()).collect();
    let t = Instant::now();
    let _ = model.encode(&texts_1000);
    println!("1000 texts: {:.3}ms ({:.2}ms each)", t.elapsed().as_secs_f64()*1000.0, t.elapsed().as_secs_f64());

    let emb_zh = model.encode_single("不要用sed修改代码");
    let emb_en = model.encode_single("Never use sed to modify source code");
    let emb_irrel = model.encode_single("今天天气真好");
    let cos = |a: &[f32], b: &[f32]| {
        let (dot, na, nb) = a.iter().zip(b).fold((0.0,0.0,0.0), |(d,na,nb), (&x,&y)| (d+x*y, na+x*x, nb+y*y));
        dot / (na.sqrt() * nb.sqrt()).max(1e-10)
    };
    println!("\nCosine similarity:");
    println!("  zh ↔ en (same meaning): {:.4}", cos(&emb_zh, &emb_en));
    println!("  zh ↔ irrelevant:        {:.4}", cos(&emb_zh, &emb_irrel));
    Ok(())
}
