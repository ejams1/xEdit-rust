{ parity probe: x-002 }
unit P_x_002;

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
  i := 7 mod 0;
  AddMessage('after');
end;
end.
