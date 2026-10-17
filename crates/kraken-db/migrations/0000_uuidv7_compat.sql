-- PostgreSQL 18 fournit uuidv7(). Pour 16 et 17 (dev local, hébergeurs en retard), une
-- fonction équivalente : 48 bits de millisecondes, version 7, puis 12 bits de fraction de
-- milliseconde (comme PG 18) pour que les ids créés dans la même milliseconde restent triés.
do $$ begin
  if current_setting('server_version_num')::int < 180000 then
    execute $f$
      create function uuidv7() returns uuid language plpgsql volatile parallel safe as $q$
      declare
        t numeric := extract(epoch from clock_timestamp()) * 1000;
        ms bigint := floor(t);
        sub int := floor((t - ms) * 4096);
        b bytea := uuid_send(gen_random_uuid());
      begin
        b := overlay(b placing substring(int8send(ms) from 3) from 1 for 6);
        b := set_byte(b, 6, 112 | (sub >> 8));
        b := set_byte(b, 7, sub & 255);
        return encode(b, 'hex')::uuid;
      end $q$
    $f$;
  end if;
end $$;
