{ parity probe: x-239 }
unit P_x_239;

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
  v := FileByIndex(0); v := Equals(v, v); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
