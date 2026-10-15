{ parity probe: x-135 }
unit P_x_135;

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
  AddMessage(IntToStr(StrToInt(5)));
  AddMessage('after');
end;
end.
