{ parity probe: x-267 }
unit P_x_267;

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
  v := FloatToStr(sl);
  AddMessage('after');
end;
end.
