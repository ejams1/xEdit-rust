{ parity probe: x-023 }
unit P_x_023;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  Foo(1);
  AddMessage('after');
end;
end.
