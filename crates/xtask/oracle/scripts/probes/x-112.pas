{ parity probe: x-112 }
unit P_x_112;

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
  i := 1; i := i + '1'; AddMessage(IntToStr(i));
  AddMessage('after');
end;
end.
