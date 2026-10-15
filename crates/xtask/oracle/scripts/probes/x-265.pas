{ parity probe: x-265 }
unit P_x_265;

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
  v := FloatToStr(True); AddMessage(v);
  AddMessage('after');
end;
end.
