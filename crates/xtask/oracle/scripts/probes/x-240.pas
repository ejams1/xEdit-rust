{ parity probe: x-240 }
unit P_x_240;

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
  v := FileByIndex(0); v := Equals(v, 5);
  AddMessage('after');
end;
end.
