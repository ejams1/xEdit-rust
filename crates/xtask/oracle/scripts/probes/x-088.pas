{ parity probe: x-088 }
unit P_x_088;

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
  AddMessage(IntToStr(Round(2.5)) + ' ' + IntToStr(Round(3.5)) + ' ' + IntToStr(Round(-2.5)));
  AddMessage('after');
end;
end.
