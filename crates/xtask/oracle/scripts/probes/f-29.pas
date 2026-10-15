{ parity probe: f-29 }
unit P_f_29;

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
  s := Format('%5.2f|%-5d|%0:s', [3.14159, 3]);
  AddMessage('[' + s + ']');
end;
end.
