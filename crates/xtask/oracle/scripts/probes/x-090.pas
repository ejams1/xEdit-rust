{ parity probe: x-090 }
unit P_x_090;

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
  AddMessage(BoolToStr(True));
  AddMessage('after');
end;
end.
