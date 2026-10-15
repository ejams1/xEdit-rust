{ parity probe: x-139 }
unit P_x_139;

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
  AddMessage(IntToStr(StrToInt('+5')));
  AddMessage('after');
end;
end.
