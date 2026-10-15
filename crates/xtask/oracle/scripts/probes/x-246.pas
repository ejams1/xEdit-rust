{ parity probe: x-246 }
unit P_x_246;

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
  v := ElementCount(Null); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
