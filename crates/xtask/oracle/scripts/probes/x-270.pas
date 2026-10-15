{ parity probe: x-270 }
unit P_x_270;

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
  v := StrToInt(1.5); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
