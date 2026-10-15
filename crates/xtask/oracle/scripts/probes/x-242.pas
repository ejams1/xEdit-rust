{ parity probe: x-242 }
unit P_x_242;

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
  v := FileByIndex(0); v := ElementCount(nil); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
