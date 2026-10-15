{ parity probe: x-141 }
unit P_x_141;

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
  AddMessage(IntToStr(StrToInt('$10')));
  AddMessage('after');
end;
end.
