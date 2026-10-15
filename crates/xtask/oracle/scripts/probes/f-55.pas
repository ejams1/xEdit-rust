{ parity probe: f-55 }
unit P_f_55;

function Initialize: integer;
var
  lst: TList;
  sl: TStringList;
  s: string;
  i64: Int64;
  c: Cardinal;
  d: Double;
begin
  lst := TList.Create;
  sl := TStringList.Create;
  sl.Add('a');
  i64 := 5;
  c := 5;
  d := 1.5;
  s := Format('%f', [922337203685478.0]);
  AddMessage('[' + s + ']');
end;
end.
