CREATE TABLE customers (
  id serial PRIMARY KEY,
  name text NOT NULL,
  email text NOT NULL,
  country text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE products (
  id serial PRIMARY KEY,
  sku text NOT NULL,
  name text NOT NULL,
  price numeric(10,2) NOT NULL,
  stock int NOT NULL
);

CREATE TABLE orders (
  id serial PRIMARY KEY,
  customer_id int NOT NULL REFERENCES customers(id),
  product_id int NOT NULL REFERENCES products(id),
  qty int NOT NULL,
  total numeric(12,2) NOT NULL,
  status text NOT NULL,
  placed_at timestamptz NOT NULL
);

INSERT INTO customers (name, email, country)
SELECT 'Customer ' || i,
       'user' || i || '@example.com',
       (ARRAY['CN','US','DE','JP','SG'])[1 + (i % 5)]
FROM generate_series(1, 120) AS i;

INSERT INTO products (sku, name, price, stock)
SELECT 'SKU-' || lpad(i::text, 4, '0'),
       'Product ' || i,
       round((random() * 900 + 10)::numeric, 2),
       (random() * 500)::int
FROM generate_series(1, 60) AS i;

INSERT INTO orders (customer_id, product_id, qty, total, status, placed_at)
SELECT 1 + (i % 120),
       1 + (i % 60),
       1 + (i % 5),
       round((random() * 2000 + 20)::numeric, 2),
       (ARRAY['pending','paid','shipped','cancelled'])[1 + (i % 4)],
       now() - (i || ' hours')::interval
FROM generate_series(1, 2000) AS i;

CREATE INDEX ON orders (customer_id);
CREATE INDEX ON orders (status);
