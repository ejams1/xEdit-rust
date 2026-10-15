{ parity probe: x-266 }
unit P_x_266;

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
  v := FloatToStr(Null); AddMessage(v);
  AddMessage('after');
end;
end.
